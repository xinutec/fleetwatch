//! Expiring mutes against a real MariaDB. Runs only when FLEETWATCH_TEST_DATABASE_URL
//! is set (see scripts/dev-db.sh); skips otherwise. Covers CRUD + validation and
//! the two overlays a mute drives: it moves a check out of the problems list (so
//! the notifier stays quiet) and stops discolouring its overview tile — without
//! touching the stored verdict.

use chrono::{Duration, Utc};
use fleetwatch::report::repo;
use fleetwatch::report::types::{CheckUpload, NewMute, ReportUpload, Verdict};
use ulid::Ulid;

mod common;

fn check(section: &str, label: &str, verdict: Verdict) -> CheckUpload {
    CheckUpload {
        section: section.into(),
        label: label.into(),
        subject: None,
        verdict,
        observed: Some("obs".into()),
        expected: None,
        value: None,
        unit: None,
        doc_ref: None,
        detail: None,
    }
}

async fn clean_mutes(pool: &sqlx::MySqlPool, source: &str) {
    sqlx::query("DELETE FROM mute WHERE source = ?")
        .bind(source)
        .execute(pool)
        .await
        .unwrap();
}

fn new_mute(source: &str, collector: &str, label: &str, hours: u32) -> NewMute {
    NewMute {
        source: source.into(),
        collector: collector.into(),
        label: label.into(),
        reason: "off on purpose".into(),
        ttl_hours: hours,
    }
}

/// A failing check with a live mute leaves `checks`, appears under `muted`, and
/// its tile stops being red — while the stored verdict stays fail.
#[tokio::test]
async fn mute_suppresses_problem_and_greens_tile() {
    let source = "test-mute-suppress";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean_mutes(&pool, source).await;

    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: "vpn-nodes".into(),
        collected_at: Utc::now(),
        duration_ms: None,
        interval_s: Some(600),
        checks: vec![
            check("wireguard", "bes", Verdict::Fail),
            check("wireguard", "isis", Verdict::Pass),
        ],
    };
    repo::ingest(&pool, source, &upload, "{}").await.unwrap();

    // problems()/mutes are GLOBAL queries; other tests seed their own bes rows,
    // so every assertion scopes to this test's source.
    let mine = |s: &str| s == source;

    // Before muting: bes is a live problem; the tile is red.
    let before = repo::problems(&pool).await.unwrap();
    assert!(
        before
            .checks
            .iter()
            .any(|c| mine(&c.source) && c.label == "bes")
    );
    assert!(
        before
            .muted
            .iter()
            .all(|m| !(mine(&m.source) && m.label == "bes"))
    );
    let tile = |o: &[fleetwatch::report::types::OverviewEntry]| {
        o.iter().find(|e| e.source == source).cloned().unwrap()
    };
    let ov = repo::overview(&pool).await.unwrap();
    assert_eq!(tile(&ov).worst, Verdict::Fail);
    assert_eq!(tile(&ov).muted, 0);

    // Mute bes.
    let mute = repo::create_mute(&pool, &new_mute(source, "vpn-nodes", "bes", 24), "pip")
        .await
        .unwrap();

    // After: bes is gone from checks, present in muted (with reason + expiry),
    // and the tile is green with muted == 1.
    let after = repo::problems(&pool).await.unwrap();
    assert!(
        after
            .checks
            .iter()
            .all(|c| !(mine(&c.source) && c.label == "bes")),
        "bes should not notify"
    );
    let m = after
        .muted
        .iter()
        .find(|m| mine(&m.source) && m.label == "bes")
        .expect("bes shown as muted");
    assert_eq!(
        m.verdict,
        Verdict::Fail,
        "stored verdict is still the truth"
    );
    assert_eq!(m.reason, "off on purpose");
    assert_eq!(m.mute_id, mute.id);

    let ov = repo::overview(&pool).await.unwrap();
    assert_eq!(tile(&ov).worst, Verdict::Pass, "muted-only fail is not red");
    assert_eq!(tile(&ov).muted, 1);
    assert_eq!(
        tile(&ov).fail,
        0,
        "the muted fail is carved out of the live count"
    );

    // The stored check row is untouched — mutes never rewrite history.
    let (stored,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM check_result WHERE source = ? AND label = 'bes' AND verdict = 'fail'",
    )
    .bind(source)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, 1);
}

/// Unmuting (deleting the mute) makes the problem reappear immediately.
#[tokio::test]
async fn unmute_restores_problem() {
    let source = "test-mute-unmute";
    let Some((pool, _guard)) = common::setup(source).await else {
        return;
    };
    clean_mutes(&pool, source).await;

    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: "vpn-nodes".into(),
        collected_at: Utc::now(),
        duration_ms: None,
        interval_s: Some(600),
        checks: vec![check("wireguard", "bes", Verdict::Fail)],
    };
    repo::ingest(&pool, source, &upload, "{}").await.unwrap();

    let mine = |s: &str| s == source;
    let mute = repo::create_mute(&pool, &new_mute(source, "vpn-nodes", "bes", 24), "pip")
        .await
        .unwrap();
    assert!(
        repo::problems(&pool)
            .await
            .unwrap()
            .checks
            .iter()
            .all(|c| !(mine(&c.source) && c.label == "bes"))
    );

    let removed = repo::delete_mute(&pool, &mute.id).await.unwrap();
    assert!(removed);
    assert!(
        repo::problems(&pool)
            .await
            .unwrap()
            .checks
            .iter()
            .any(|c| mine(&c.source) && c.label == "bes"),
        "bes is a live problem again once unmuted"
    );
    // Deleting an unknown id is a no-op false (routes turn that into 404).
    assert!(!repo::delete_mute(&pool, "nope").await.unwrap());
}

/// An expired mute is neither listed nor applied — the suppression self-heals.
#[tokio::test]
async fn expired_mute_is_inert() {
    let source = "test-mute-expired";
    let Some((pool, _guard)) = common::setup(source).await else {
        return;
    };
    clean_mutes(&pool, source).await;

    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: "vpn-nodes".into(),
        collected_at: Utc::now(),
        duration_ms: None,
        interval_s: Some(600),
        checks: vec![check("wireguard", "bes", Verdict::Fail)],
    };
    repo::ingest(&pool, source, &upload, "{}").await.unwrap();

    // Insert a mute that already lapsed an hour ago.
    let past = (Utc::now() - Duration::hours(1)).naive_utc();
    let created = (Utc::now() - Duration::hours(2)).naive_utc();
    sqlx::query(
        "INSERT INTO mute (id, source, collector, label, reason, created_by, created_at, expires_at) \
         VALUES (?, ?, 'vpn-nodes', 'bes', 'lapsed', 'pip', ?, ?)",
    )
    .bind(Ulid::new().to_string())
    .bind(source)
    .bind(created)
    .bind(past)
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        repo::list_mutes(&pool)
            .await
            .unwrap()
            .iter()
            .all(|m| m.source != source)
    );
    let p = repo::problems(&pool).await.unwrap();
    assert!(
        p.checks
            .iter()
            .any(|c| c.source == source && c.label == "bes"),
        "expired mute must not suppress"
    );
    assert!(p.muted.iter().all(|m| m.source != source));
    let ov = repo::overview(&pool).await.unwrap();
    let tile = ov.iter().find(|e| e.source == source).unwrap();
    assert_eq!(tile.worst, Verdict::Fail);
    assert_eq!(tile.muted, 0);
}

/// Validation: identity and reason are mandatory; ttl is clamped, never zero.
#[tokio::test]
async fn create_mute_validates() {
    let source = "test-mute-validate";
    let Some((pool, _guard)) = common::setup(source).await else {
        return;
    };
    clean_mutes(&pool, source).await;

    let empty_reason = NewMute {
        reason: "  ".into(),
        ..new_mute(source, "vpn-nodes", "bes", 24)
    };
    assert!(
        repo::create_mute(&pool, &empty_reason, "pip")
            .await
            .is_err()
    );

    let empty_label = new_mute(source, "vpn-nodes", "  ", 24);
    assert!(repo::create_mute(&pool, &empty_label, "pip").await.is_err());

    // ttl 0 clamps up to the 1-hour floor — the mute is live now.
    let clamped = repo::create_mute(&pool, &new_mute(source, "vpn-nodes", "bes", 0), "pip")
        .await
        .unwrap();
    assert!(clamped.expires_at > Utc::now());
    assert!(
        repo::list_mutes(&pool)
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == clamped.id)
    );
}
