//! The `latest_report` pointer: the stored answer to "newest report per
//! (source, collector)", which `overview()`/`problems()` join instead of ranking
//! the whole `report` table per request.
//!
//! These tests pin the pointer to the ordering it replaced — newer
//! `collected_at` wins, ties broken by the larger id — because the two are only
//! interchangeable while they agree. Runs only when FLEETWATCH_TEST_DATABASE_URL
//! is set; skips otherwise, like the other DB tests.

use chrono::{DateTime, Duration, SubsecRound, Utc};
use fleetwatch::report::repo;
use fleetwatch::report::types::{CheckUpload, ReportUpload, Verdict};
use sqlx::MySqlPool;
use ulid::Ulid;

mod common;

fn one_check() -> CheckUpload {
    CheckUpload {
        section: "s".into(),
        label: "l".into(),
        subject: None,
        verdict: Verdict::Pass,
        observed: None,
        expected: None,
        value: None,
        unit: None,
        doc_ref: None,
        detail: None,
    }
}

/// A collection time `n` minutes ago, truncated to the DATETIME(3) the column
/// stores — so a timestamp two reports share is a tie in the database too.
///
/// Callers that mean "the same instant" must pass ONE of these to both ingests.
/// Recomputing it per call drifts by the milliseconds between them, which turns
/// an intended tie into an ordinary newer-report case and quietly tests nothing.
fn minutes_ago(n: i64) -> DateTime<Utc> {
    (Utc::now() - Duration::minutes(n)).trunc_subsecs(3)
}

/// Ingest one report with an explicit id and collection time.
async fn ingest_at(
    pool: &MySqlPool,
    source: &str,
    collector: &str,
    id: &str,
    collected_at: DateTime<Utc>,
) -> String {
    let upload = ReportUpload {
        schema: 1,
        id: id.into(),
        collector: collector.into(),
        collected_at,
        duration_ms: None,
        interval_s: Some(3600),
        checks: vec![one_check()],
    };
    repo::ingest(pool, source, &upload, "{}")
        .await
        .expect("ingest");
    id.into()
}

async fn pointer(pool: &MySqlPool, source: &str, collector: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT report_id FROM latest_report WHERE source = ? AND collector = ?",
    )
    .bind(source)
    .bind(collector)
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|(id,)| id)
}

/// The ranking the pointer replaced, run over this source only. The pointer is
/// correct exactly when it equals this.
async fn derived(pool: &MySqlPool, source: &str, collector: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT id FROM (SELECT id, ROW_NUMBER() OVER \
           (PARTITION BY source, collector ORDER BY collected_at DESC, id DESC) rn \
         FROM report WHERE source = ? AND collector = ?) x WHERE rn = 1",
    )
    .bind(source)
    .bind(collector)
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|(id,)| id)
}

/// Two ULIDs, smaller first. Minting is time-ordered, so this is just a sort —
/// but the tests below depend on WHICH is smaller, not on how they were made.
fn id_pair() -> (String, String) {
    let (a, b) = (Ulid::new().to_string(), Ulid::new().to_string());
    if a < b { (a, b) } else { (b, a) }
}

#[tokio::test]
async fn pointer_follows_collection_time_not_arrival_order() {
    let source = "test-latest-order";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping latest_report test");
        return;
    };
    let collector = "c";

    // Arrive middle, newest, oldest — a spool draining after a network flap.
    ingest_at(
        &pool,
        source,
        collector,
        &Ulid::new().to_string(),
        minutes_ago(120),
    )
    .await;
    let newest = ingest_at(
        &pool,
        source,
        collector,
        &Ulid::new().to_string(),
        minutes_ago(60),
    )
    .await;
    ingest_at(
        &pool,
        source,
        collector,
        &Ulid::new().to_string(),
        minutes_ago(180),
    )
    .await;

    assert_eq!(
        pointer(&pool, source, collector).await.as_deref(),
        Some(&*newest)
    );
    assert_eq!(
        derived(&pool, source, collector).await,
        pointer(&pool, source, collector).await
    );

    // A replay of the newest is a no-op that leaves the pointer where it is.
    let replay = ReportUpload {
        schema: 1,
        id: newest.clone(),
        collector: collector.into(),
        collected_at: Utc::now(),
        duration_ms: None,
        interval_s: Some(3600),
        checks: vec![one_check()],
    };
    assert!(
        repo::ingest(&pool, source, &replay, "{}")
            .await
            .unwrap()
            .duplicate
    );
    assert_eq!(
        pointer(&pool, source, collector).await.as_deref(),
        Some(&*newest)
    );

    // And the view agrees with the pointer.
    let entry = repo::overview(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.source == source)
        .expect("overview entry");
    assert_eq!(entry.report_id, newest);

    common::clean(&pool, source).await;
}

/// The regression this test exists for: `collected_at` and `report_id` are
/// updated in one statement, and the id's comparison must read the OLD
/// `collected_at`. Assign `collected_at` first and a newer report whose id
/// happens to sort LOWER stops taking the pointer — silently, and only for
/// producers whose clock and ULID minting disagree.
#[tokio::test]
async fn a_newer_report_wins_even_with_a_lower_id() {
    let source = "test-latest-lower-id";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping latest_report test");
        return;
    };
    let collector = "c";
    let (lo, hi) = id_pair();

    ingest_at(&pool, source, collector, &hi, minutes_ago(120)).await; // higher id, older
    ingest_at(&pool, source, collector, &lo, minutes_ago(10)).await; //  lower id, newer

    assert_eq!(
        pointer(&pool, source, collector).await.as_deref(),
        Some(&*lo)
    );
    assert_eq!(
        derived(&pool, source, collector).await,
        pointer(&pool, source, collector).await
    );

    common::clean(&pool, source).await;
}

/// Same collection time: the larger id wins, whichever arrives first.
#[tokio::test]
async fn a_tie_on_collected_at_is_broken_by_the_larger_id() {
    let source = "test-latest-tie";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping latest_report test");
        return;
    };

    // ONE instant, shared by both reports in each pair — that is the tie.
    let tie = minutes_ago(30);

    let (lo, hi) = id_pair();
    ingest_at(&pool, source, "lo-first", &lo, tie).await;
    ingest_at(&pool, source, "lo-first", &hi, tie).await;
    assert_eq!(
        pointer(&pool, source, "lo-first").await.as_deref(),
        Some(&*hi)
    );

    let (lo2, hi2) = id_pair();
    ingest_at(&pool, source, "hi-first", &hi2, tie).await;
    ingest_at(&pool, source, "hi-first", &lo2, tie).await;
    assert_eq!(
        pointer(&pool, source, "hi-first").await.as_deref(),
        Some(&*hi2)
    );

    for collector in ["lo-first", "hi-first"] {
        assert_eq!(
            derived(&pool, source, collector).await,
            pointer(&pool, source, collector).await,
            "{collector}: pointer disagrees with the ranking it replaced"
        );
    }

    common::clean(&pool, source).await;
}

/// The repair path: whatever state the table is in, rebuilding from `report`
/// reproduces the ranking. Guards the backfill in migration 0005, which is this
/// same SQL.
#[tokio::test]
async fn rebuild_reproduces_the_pointer_from_history() {
    let source = "test-latest-rebuild";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping latest_report test");
        return;
    };
    let collector = "c";

    ingest_at(
        &pool,
        source,
        collector,
        &Ulid::new().to_string(),
        minutes_ago(90),
    )
    .await;
    let newest = ingest_at(
        &pool,
        source,
        collector,
        &Ulid::new().to_string(),
        minutes_ago(5),
    )
    .await;

    // Corrupt the pointer, then repair it.
    sqlx::query("DELETE FROM latest_report WHERE source = ?")
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(pointer(&pool, source, collector).await, None);

    repo::rebuild_latest_report(&pool).await.expect("rebuild");
    assert_eq!(
        pointer(&pool, source, collector).await.as_deref(),
        Some(&*newest)
    );

    common::clean(&pool, source).await;
}
