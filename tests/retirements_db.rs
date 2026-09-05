//! Retiring a producer, against a real MariaDB. Runs only when
//! FLEETWATCH_TEST_DATABASE_URL is set (see scripts/dev-db.sh); skips otherwise.
//!
//! Covers the two things a retirement means and the one thing it must not do:
//! a retired producer stops counting as stale, a retired producer REPORTING
//! AGAIN is surfaced loudly rather than silently un-retired, and neither path
//! touches a single stored row.

use chrono::{DateTime, Duration, Utc};
use fleetwatch::report::repo;
use fleetwatch::report::types::{CheckUpload, NewRetirement, ReportUpload, Verdict};
use ulid::Ulid;

mod common;

fn check(label: &str, verdict: Verdict) -> CheckUpload {
    CheckUpload {
        section: "s".into(),
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

async fn clean(pool: &sqlx::MySqlPool, source: &str) {
    sqlx::query("DELETE FROM retirement WHERE source = ?")
        .bind(source)
        .execute(pool)
        .await
        .unwrap();
}

/// A report collected at an exact instant.
///
/// ⚠ Use this, not `ingest_aged`, for anything about `returned`. That list is
/// `collected_at > retired_at`, so a test of it needs the report placed relative
/// to THE RETIREMENT — and `Utc::now()` only places it relative to whenever the
/// test reached that line. Both columns are `DATETIME(3)`, so "now, just after
/// retiring" is the same millisecond as the retirement whenever the intervening
/// INSERT and read-back take under 1 ms, which on this machine is often: the two
/// tests below failed **7 runs in 20 on an IDLE machine** before they took their
/// timestamps from the retirement instead.
async fn ingest_at(
    pool: &sqlx::MySqlPool,
    source: &str,
    collector: &str,
    collected_at: DateTime<Utc>,
) {
    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: collector.into(),
        collected_at,
        duration_ms: None,
        interval_s: Some(600),
        checks: vec![check("a", Verdict::Pass)],
    };
    repo::ingest(pool, source, &upload, "{}").await.unwrap();
}

/// A report whose `collected_at` is `age` in the past, so freshness is a
/// property of the fixture rather than of how long the test took to run.
async fn ingest_aged(pool: &sqlx::MySqlPool, source: &str, collector: &str, age: Duration) {
    ingest_at(pool, source, collector, Utc::now() - age).await
}

/// The gap this whole feature exists for: a producer that moved host keeps its
/// last report for ever and `overview()` calls it silent for ever. Retiring it
/// takes it out of `stale` — and the report rows stay exactly where they were.
#[tokio::test]
async fn retiring_a_producer_removes_it_from_stale_without_deleting_history() {
    let source = "test-retire-stale";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;

    // 600s interval, 10h old: far past 3 × interval, so `silent`.
    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;

    let mine = |s: &str| s == source;
    let before = repo::problems(&pool).await.unwrap();
    assert!(
        before.stale.iter().any(|e| mine(&e.source)),
        "precondition: the producer must be stale before it is retired"
    );
    let rows_before = repo::list_reports(&pool, Some(source), None, 100)
        .await
        .unwrap()
        .len();

    repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "collector moved amun -> isis".into(),
        },
        "pippijn",
    )
    .await
    .unwrap();

    let after = repo::problems(&pool).await.unwrap();
    assert!(
        !after.stale.iter().any(|e| mine(&e.source)),
        "a retired producer must not count as stale"
    );
    assert!(
        !after.returned.iter().any(|e| mine(&e.source)),
        "it has not reported since being retired, so it has not returned"
    );

    // The whole point: silence WITHOUT discarding what it measured. The manual
    // DELETE this replaces took 1,757 report rows and 61,468 check rows.
    let rows_after = repo::list_reports(&pool, Some(source), None, 100)
        .await
        .unwrap()
        .len();
    assert_eq!(
        rows_before, rows_after,
        "retirement is a read-time overlay; it must not delete history"
    );
}

/// The decision recorded in migration 0007: a retired producer that reports
/// again is LOUD, not silently un-retired. Convenience would be to un-retire;
/// being loud is what catches a host coming back that nobody meant to restart.
#[tokio::test]
async fn a_retired_producer_that_reports_again_is_loud() {
    let source = "test-retire-returned";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;

    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;
    let retirement = repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "decommissioned".into(),
        },
        "pippijn",
    )
    .await
    .unwrap();

    // It comes back — ONE MILLISECOND after the retirement, which is the
    // smallest gap `DATETIME(3)` can hold. Taking the instant from the
    // retirement rather than from `Utc::now()` is what makes this a test of
    // `returned` and not of how fast MariaDB answered: a report timestamped
    // even slightly in the PAST is correctly not a return, which is pinned in
    // `a_report_collected_before_the_retirement_is_not_a_return`, and one
    // timestamped in the same millisecond is pinned in `not_a_return` below.
    ingest_at(
        &pool,
        source,
        "moved",
        retirement.retired_at + Duration::milliseconds(1),
    )
    .await;

    let mine = |s: &str| s == source;
    let after = repo::problems(&pool).await.unwrap();
    assert!(
        after.returned.iter().any(|e| mine(&e.source)),
        "a retired producer reporting again must be announced"
    );
    assert!(
        !after.stale.iter().any(|e| mine(&e.source)),
        "and it is not stale — it just reported, which is exactly why `stale` \
         could never have carried this case"
    );
    assert!(
        repo::list_retirements(&pool)
            .await
            .unwrap()
            .iter()
            .any(|r| mine(&r.source)),
        "it must NOT have un-retired itself — that would make the return silent"
    );
}

/// Re-retiring must not move `retired_at` forward. That timestamp is what
/// `returned` compares against, so resetting it would forgive a return that had
/// already happened — the producer would go quiet again on the next read.
#[tokio::test]
async fn re_retiring_keeps_the_original_timestamp() {
    let source = "test-retire-idempotent";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;
    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;

    let new = |reason: &str| NewRetirement {
        source: source.into(),
        collector: "moved".into(),
        reason: reason.into(),
    };
    let first = repo::create_retirement(&pool, &new("moved host"), "pippijn")
        .await
        .unwrap();

    // It comes back, then somebody retires it again without noticing. The
    // return is placed one millisecond after the retirement for the reason
    // `ingest_at` gives — `Utc::now()` here lands in the retirement's own
    // millisecond often enough to fail better than one run in three.
    ingest_at(
        &pool,
        source,
        "moved",
        first.retired_at + Duration::milliseconds(1),
    )
    .await;
    let second = repo::create_retirement(&pool, &new("moved host, again"), "pippijn")
        .await
        .unwrap();

    assert_eq!(
        first.retired_at, second.retired_at,
        "re-retiring must keep the original timestamp"
    );
    assert_eq!(
        second.reason, "moved host, again",
        "but the reason restates"
    );
    assert!(
        repo::problems(&pool)
            .await
            .unwrap()
            .returned
            .iter()
            .any(|e| e.source == source),
        "the return must still be announced after a re-retire"
    );
}

/// Un-retiring restores staleness on the next read, and reports 404-shaped
/// `false` when there was nothing to remove.
#[tokio::test]
async fn un_retiring_restores_staleness() {
    let source = "test-retire-undo";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;
    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;

    assert!(
        !repo::delete_retirement(&pool, source, "moved")
            .await
            .unwrap(),
        "nothing to un-retire yet"
    );

    repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "gone".into(),
        },
        "pippijn",
    )
    .await
    .unwrap();
    assert!(
        repo::delete_retirement(&pool, source, "moved")
            .await
            .unwrap()
    );

    assert!(
        repo::problems(&pool)
            .await
            .unwrap()
            .stale
            .iter()
            .any(|e| e.source == source),
        "un-retired, so it counts as stale again"
    );
}

/// A retirement without a reason is refused, for the same purpose a mute's
/// mandatory reason serves: a producer dropped from the fleet's attention with
/// no attributable reason is the blind spot this design exists to avoid.
#[tokio::test]
async fn a_retirement_needs_a_reason() {
    let source = "test-retire-reason";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;

    let err = repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "   ".into(),
        },
        "pippijn",
    )
    .await;
    assert!(err.is_err(), "a blank reason must be refused");
}

/// A report COLLECTED before the retirement but ingested after it is not a
/// return. The producer was alive before it was declared finished, which is not
/// news; treating it as a return would announce every report still in flight
/// when somebody retires a producer.
#[tokio::test]
async fn a_report_collected_before_the_retirement_is_not_a_return() {
    let source = "test-retire-inflight";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;
    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;

    repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "gone".into(),
        },
        "pippijn",
    )
    .await
    .unwrap();

    // Collected an hour ago; arrives now, after the retirement was recorded.
    ingest_aged(&pool, source, "moved", Duration::hours(1)).await;

    let p = repo::problems(&pool).await.unwrap();
    assert!(
        !p.returned.iter().any(|e| e.source == source),
        "a report collected before the retirement is not a return"
    );
    assert!(
        !p.stale.iter().any(|e| e.source == source),
        "and it is still retired, so it is not stale either"
    );
}

/// A report collected in the SAME MILLISECOND as the retirement is not a return.
///
/// This is the boundary, and it is stated here because not stating it cost real
/// time: `returned` is `collected_at > retired_at` over two `DATETIME(3)`
/// columns, so the finest distinction either can make is a millisecond, and
/// equal is not greater. Two tests above used to say "retire, then report NOW"
/// and landed inside that millisecond whenever the intervening round trip was
/// quick — 7 failures in 20 runs on an idle machine, read for a day as a
/// concurrency problem because the one re-run that acquitted it happened to
/// pass.
///
/// The behaviour itself is right: at equal timestamps nothing distinguishes a
/// report that arrived just before the retirement from one just after, and
/// announcing a return that may not have happened is the louder mistake. What
/// was wrong was a test asserting on an ordering it did not set.
#[tokio::test]
async fn a_report_collected_in_the_retirements_own_millisecond_is_not_a_return() {
    let source = "test-retire-sameinstant";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    clean(&pool, source).await;
    ingest_aged(&pool, source, "moved", Duration::hours(10)).await;

    let retirement = repo::create_retirement(
        &pool,
        &NewRetirement {
            source: source.into(),
            collector: "moved".into(),
            reason: "gone".into(),
        },
        "pippijn",
    )
    .await
    .unwrap();

    ingest_at(&pool, source, "moved", retirement.retired_at).await;

    let p = repo::problems(&pool).await.unwrap();
    assert!(
        !p.returned.iter().any(|e| e.source == source),
        "equal is not greater — the same millisecond is not a return"
    );

    // And one millisecond later IS, so the assertion above is about the
    // boundary rather than about `returned` being empty for some other reason.
    ingest_at(
        &pool,
        source,
        "moved",
        retirement.retired_at + Duration::milliseconds(1),
    )
    .await;
    assert!(
        repo::problems(&pool)
            .await
            .unwrap()
            .returned
            .iter()
            .any(|e| e.source == source),
        "one millisecond past the retirement is a return"
    );
}
