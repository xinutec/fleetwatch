//! How long a check has been failing — the age `problems()` could not report.
//!
//! ⚠ These pin the RUN, not the report. A fault that persists across many
//! reports has ONE start, and a fault that clears and returns has TWO — the
//! second age describes the second fault, not the first. That distinction is the
//! whole value: "red for eight days" and "red since the last run" want different
//! responses, and before migration 0006 they were indistinguishable.

use chrono::{Duration, Utc};
use fleetwatch::report::repo;
use fleetwatch::report::types::{CheckUpload, ReportUpload, Verdict};
use ulid::Ulid;

mod common;

fn check(label: &str, verdict: Verdict) -> CheckUpload {
    CheckUpload {
        section: "s".into(),
        label: label.into(),
        subject: None,
        verdict,
        observed: None,
        expected: None,
        value: None,
        unit: None,
        doc_ref: None,
        detail: None,
    }
}

async fn ingest(pool: &sqlx::MySqlPool, source: &str, ago_min: i64, checks: Vec<CheckUpload>) {
    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: "c".into(),
        collected_at: Utc::now() - Duration::minutes(ago_min),
        duration_ms: None,
        interval_s: Some(3600),
        checks,
    };
    repo::ingest(pool, source, &upload, "{}")
        .await
        .expect("ingest");
}

async fn first_seen_of(pool: &sqlx::MySqlPool, source: &str, label: &str) -> Option<i64> {
    repo::problems(pool)
        .await
        .unwrap()
        .checks
        .into_iter()
        .find(|c| c.source == source && c.label == label)
        .and_then(|c| c.first_seen)
        .map(|t| (Utc::now() - t).num_minutes())
}

/// A fault spanning several reports keeps ONE start — the first one.
#[tokio::test]
async fn a_persisting_fault_keeps_the_age_of_its_first_report() {
    let source = "test-since-persist";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping problem_since test");
        return;
    };

    ingest(&pool, source, 180, vec![check("stuck", Verdict::Fail)]).await;
    ingest(&pool, source, 120, vec![check("stuck", Verdict::Fail)]).await;
    ingest(&pool, source, 60, vec![check("stuck", Verdict::Fail)]).await;

    let age = first_seen_of(&pool, source, "stuck")
        .await
        .expect("a first_seen");
    assert!(
        (175..=185).contains(&age),
        "should date from the FIRST failing report (~180m), got {age}m — a later \
         report has pushed the start forward, which would make every fault look new"
    );

    common::clean(&pool, source).await;
}

/// Passing ends the run; the next failure starts a new one.
#[tokio::test]
async fn a_fault_that_clears_and_returns_gets_a_new_age() {
    let source = "test-since-cleared";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping problem_since test");
        return;
    };

    ingest(&pool, source, 300, vec![check("flapper", Verdict::Fail)]).await;
    ingest(&pool, source, 240, vec![check("flapper", Verdict::Pass)]).await;
    ingest(&pool, source, 30, vec![check("flapper", Verdict::Fail)]).await;

    let age = first_seen_of(&pool, source, "flapper")
        .await
        .expect("a first_seen");
    assert!(
        (25..=35).contains(&age),
        "the age should describe THIS fault (~30m), not the one that cleared five \
         hours ago, got {age}m"
    );

    common::clean(&pool, source).await;
}

/// ⚠ THE ORDERING GUARD. A spool draining after a network flap replays older
/// reports. An old report showing the check PASSING must not close a run that is
/// still open — that would reset the age of a fault which never went away, and
/// the reset would look exactly like the fault having just started.
#[tokio::test]
async fn a_late_replay_of_an_older_report_does_not_reset_the_age() {
    let source = "test-since-replay";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping problem_since test");
        return;
    };

    // Failing since three hours ago, still failing at the latest report.
    ingest(&pool, source, 180, vec![check("held", Verdict::Fail)]).await;
    ingest(&pool, source, 10, vec![check("held", Verdict::Fail)]).await;

    // Now a stale report from four hours ago arrives, when it was passing.
    ingest(&pool, source, 240, vec![check("held", Verdict::Pass)]).await;

    let age = first_seen_of(&pool, source, "held")
        .await
        .expect("still failing");
    assert!(
        (175..=185).contains(&age),
        "a replayed OLD report closed a run that is still open: age is {age}m, \
         expected ~180m"
    );

    common::clean(&pool, source).await;
}
