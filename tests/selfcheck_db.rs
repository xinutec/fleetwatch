//! fleetwatch's self-report against a real MariaDB. Runs only when
//! FLEETWATCH_TEST_DATABASE_URL is set (see scripts/dev-db.sh); skips otherwise.
//!
//! The pure judgements are pinned in `tests/selfcheck.rs`. What needs a database
//! is the shape of the report it assembles: one lag row per producer, its own
//! row left out, and the whole thing surviving a round trip through the same
//! ingest path every other producer uses.

use chrono::{Duration, Utc};
use fleetwatch::report::repo;
use fleetwatch::report::types::{CheckUpload, ReportUpload, Verdict};
use fleetwatch::selfcheck::{self, Boot, Sweep};
use ulid::Ulid;

mod common;

const BOOT: Boot = Boot {
    migrate_ms: 47_000,
    reconcile_ms: 1_112,
};

async fn ingest(pool: &sqlx::MySqlPool, source: &str, collector: &str, collected_ago: Duration) {
    let upload = ReportUpload {
        schema: 1,
        id: Ulid::new().to_string(),
        collector: collector.into(),
        collected_at: Utc::now() - collected_ago,
        duration_ms: None,
        interval_s: Some(600),
        checks: vec![CheckUpload {
            section: "s".into(),
            label: "a".into(),
            subject: None,
            verdict: Verdict::Pass,
            observed: None,
            expected: None,
            value: None,
            unit: None,
            doc_ref: None,
            detail: None,
        }],
    };
    repo::ingest(pool, source, &upload, "{}").await.unwrap();
}

/// The report carries a lag row for each producer, and NOT one for itself — its
/// own row is written by this process at the moment it is collected, so it would
/// always read zero, and a zero nobody can act on is noise on a page whose job
/// is to be scanned.
#[tokio::test]
async fn the_self_report_covers_every_producer_but_itself() {
    let source = "test-selfcheck";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    ingest(&pool, source, "alpha", Duration::seconds(30)).await;
    ingest(&pool, source, "beta", Duration::seconds(30)).await;

    // Give it a self-row to ignore, through the real ingest path.
    selfcheck::run_once(&pool, BOOT, None).await.unwrap();

    let report = selfcheck::build(&pool, BOOT, None).await.unwrap();
    let lag_labels: Vec<&str> = report
        .checks
        .iter()
        .filter(|c| c.section == "ingest lag")
        .map(|c| c.label.as_str())
        .collect();

    assert!(
        lag_labels.iter().any(|l| l.contains("alpha")),
        "{lag_labels:?}"
    );
    assert!(
        lag_labels.iter().any(|l| l.contains("beta")),
        "{lag_labels:?}"
    );
    assert!(
        !lag_labels
            .iter()
            .any(|l| l.contains(selfcheck::COLLECTOR) && l.contains(selfcheck::SOURCE)),
        "it must not report its own ingest lag: {lag_labels:?}"
    );
}

/// It stores through the SAME path as every other producer, so it appears on the
/// dashboard as an ordinary tile rather than needing a second rendering.
#[tokio::test]
async fn the_self_report_lands_as_an_ordinary_report() {
    let source = "test-selfcheck-lands";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };
    // Clear any self-rows a previous run left, so the assertion is about THIS run.
    sqlx::query("DELETE FROM report WHERE source = ?")
        .bind(selfcheck::SOURCE)
        .execute(&pool)
        .await
        .unwrap();

    selfcheck::run_once(&pool, BOOT, None).await.unwrap();

    let mine = repo::list_reports(&pool, Some(selfcheck::SOURCE), None, 10)
        .await
        .unwrap();
    let latest = mine.first().expect("a self-report must have been stored");
    assert_eq!(latest.collector, selfcheck::COLLECTOR);
    assert!(latest.pass + latest.warn + latest.fail + latest.skip >= 4);
}

/// Boot costs and the retention outcome ride on every report, because they are
/// the two facts that had nowhere durable to live at all.
#[tokio::test]
async fn boot_and_retention_facts_are_carried_on_every_report() {
    let source = "test-selfcheck-facts";
    let Some((pool, _guard)) = common::setup(source).await else {
        eprintln!("FLEETWATCH_TEST_DATABASE_URL unset — skipping");
        return;
    };

    let swept = Sweep {
        at: Utc::now() - Duration::hours(2),
        raw_cleared: 477,
        checks_deleted: 0,
    };
    let report = selfcheck::build(&pool, BOOT, Some(swept)).await.unwrap();

    let by_label = |l: &str| {
        report
            .checks
            .iter()
            .find(|c| c.label == l)
            .unwrap_or_else(|| panic!("missing check {l}"))
            .clone()
    };
    assert_eq!(by_label("migrations").value, Some(47_000.0));
    assert_eq!(by_label("latest_report reconcile").value, Some(1_112.0));

    let retention = by_label("last sweep");
    assert_eq!(retention.verdict, Verdict::Pass);
    assert!(retention.observed.unwrap().contains("477"));

    // And the pool row is always present — it is the one number with no query.
    assert!(report.checks.iter().any(|c| c.section == "pool"));
}
