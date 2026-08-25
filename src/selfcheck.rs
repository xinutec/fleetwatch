//! fleetwatch reporting on ITSELF, as a producer like any other.
//!
//! ⚠ THIS IS THE WEAKER HALF ON PURPOSE, and the strong half already exists.
//! `mac-mini/fleet_health.py` fetches `/api/problems` from OUTSIDE the cluster
//! and validates the payload, so a dead or misrouted fleetwatch is caught by
//! something that does not depend on fleetwatch being alive. A monitor
//! reporting on itself is silent in exactly the case that matters, and nothing
//! here changes that. What this adds is the internal state no external probe
//! can reach.
//!
//! Four facts, each already computed or one query away (#1069 draws that line
//! explicitly: anything needing new instrumentation is a different ticket):
//!
//! * **pool utilisation** — #1053 was pool exhaustion and the only symptom was
//!   500s. ⚠ This SAMPLES the pool when the report is built, so it shows the
//!   trend and will miss a spike between samples. Catching the spike itself
//!   needs instrumentation on acquire, which is the separate ticket.
//! * **ingest lag** — `received_at - collected_at`, per producer. Distinct from
//!   staleness, which is `now - collected_at`: a producer that collects on time
//!   but cannot upload has a growing lag, and staleness conflates that with a
//!   producer that stopped collecting. Separating them names which half broke.
//! * **retention outcomes** — the daily sweep logged to stdout and nowhere
//!   durable, so a sweep that stopped running was invisible until the disk
//!   filled.
//! * **boot cost** — migration 0006 took 47s against a stated 0.54s and only
//!   the pod log said so.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::MySqlPool;
use tokio::sync::RwLock;
use ulid::Ulid;

use crate::db::MAX_CONNECTIONS;
use crate::report::repo;
use crate::report::types::{CheckUpload, ReportUpload, Verdict};

/// The source this reports under. Its own, rather than `isis`: the subject is
/// the service, not the machine, and a reader looking at why fleetwatch is slow
/// should not have to know where it happens to run.
pub const SOURCE: &str = "fleetwatch";
pub const COLLECTOR: &str = "internals";

/// Every 15 minutes. Frequent enough for pool and lag to show a trend, rare
/// enough not to dominate its own `report` table — at 96 reports a day this is
/// the busiest producer in the fleet, and that is the cost of it being the only
/// view of these numbers.
pub const INTERVAL: Duration = Duration::from_secs(900);

/// Pool pressure worth saying something about. Exhaustion is the fault (#1053);
/// three-quarters is the warning that precedes it, and it is a proportion
/// rather than a count so it survives `MAX_CONNECTIONS` changing.
const POOL_WARN_FRACTION: f64 = 0.75;

/// What the last retention sweep did. Written by the sweeper, read here — the
/// numbers existed already and went only to stdout.
#[derive(Debug, Clone, Copy)]
pub struct Sweep {
    pub at: DateTime<Utc>,
    pub raw_cleared: u64,
    pub checks_deleted: u64,
}

/// One-off costs measured during startup, kept so they can be reported later.
#[derive(Debug, Clone, Copy)]
pub struct Boot {
    pub migrate_ms: u64,
    pub reconcile_ms: u64,
}

/// Shared handle to the last sweep. `None` until the first one completes, which
/// is within seconds of boot because the sweeper's first tick fires immediately.
pub type SweepState = Arc<RwLock<Option<Sweep>>>;

fn check(section: &str, label: &str, verdict: Verdict, observed: String) -> CheckUpload {
    CheckUpload {
        section: section.into(),
        label: label.into(),
        subject: None,
        verdict,
        observed: Some(observed),
        expected: None,
        value: None,
        unit: None,
        doc_ref: None,
        detail: None,
    }
}

fn valued(mut c: CheckUpload, value: f64, unit: &str) -> CheckUpload {
    c.value = Some(value);
    c.unit = Some(unit.into());
    c
}

/// Pool utilisation, from the pool's own counters — no query.
pub fn pool_check(size: u32, idle: usize) -> CheckUpload {
    // `size` is connections the pool has OPENED, not the ceiling; in-use is what
    // is not idle among them. A pool that has never been under load reports a
    // small size and zero pressure, which is the honest answer.
    let in_use = size.saturating_sub(idle as u32);
    let fraction = f64::from(in_use) / f64::from(MAX_CONNECTIONS);
    let verdict = if in_use >= MAX_CONNECTIONS {
        Verdict::Fail
    } else if fraction >= POOL_WARN_FRACTION {
        Verdict::Warn
    } else {
        Verdict::Pass
    };
    valued(
        check(
            "pool",
            "connections in use",
            verdict,
            format!("{in_use} of {MAX_CONNECTIONS} in use, {idle} idle, {size} open"),
        ),
        f64::from(in_use),
        "connections",
    )
}

/// One check per producer: how long its newest report took to arrive.
///
/// ⚠ The threshold is the producer's OWN `interval_s`, not a number chosen here.
/// A report that took longer to arrive than the producer's collection period
/// means the spool is falling behind faster than it drains, which is true
/// whatever that period is — self-calibrating, and it cannot rot the way a
/// magic constant does when a producer's schedule changes.
pub fn lag_check(
    source: &str,
    collector: &str,
    lag_s: i64,
    interval_s: Option<u64>,
) -> CheckUpload {
    let verdict = match interval_s {
        Some(i) if lag_s > i as i64 => Verdict::Warn,
        // No declared interval means no self-calibrating threshold, so this
        // reports the number and judges nothing. Inventing one here would be a
        // magic constant wearing a producer's name.
        _ => Verdict::Pass,
    };
    let expectation = match interval_s {
        Some(i) => format!(", its collection period is {i}s"),
        None => ", no declared interval".into(),
    };
    valued(
        check(
            "ingest lag",
            &format!("{source} · {collector}"),
            verdict,
            format!("newest report arrived {lag_s}s after it was collected{expectation}"),
        ),
        lag_s as f64,
        "s",
    )
}

/// Did the daily sweep run, and what did it clear?
pub fn retention_check(sweep: Option<Sweep>, now: DateTime<Utc>) -> CheckUpload {
    let Some(s) = sweep else {
        return check(
            "retention",
            "last sweep",
            Verdict::Skip,
            "no sweep has completed since this pod started".into(),
        );
    };
    let age_s = (now - s.at).num_seconds();
    // The sweeper ticks every 24h. A window at exactly 24h would fire on
    // ordinary jitter, so this allows an hour of it and no more — long enough
    // to be quiet in the steady state, short enough that a sweeper which has
    // stopped is named within one cycle rather than at disk-full.
    let verdict = if age_s > 25 * 3600 {
        Verdict::Fail
    } else {
        Verdict::Pass
    };
    valued(
        check(
            "retention",
            "last sweep",
            verdict,
            format!(
                "{}h ago: cleared {} raw payload(s), deleted {} old check(s)",
                age_s / 3600,
                s.raw_cleared,
                s.checks_deleted
            ),
        ),
        age_s as f64,
        "s",
    )
}

/// What boot cost. Reported, not judged: the complaint in #1069 is that 47s of
/// migration was invisible, not that 47s was wrong.
pub fn boot_checks(boot: Boot) -> Vec<CheckUpload> {
    vec![
        valued(
            check(
                "boot",
                "migrations",
                Verdict::Pass,
                format!("{}ms to apply", boot.migrate_ms),
            ),
            boot.migrate_ms as f64,
            "ms",
        ),
        valued(
            check(
                "boot",
                "latest_report reconcile",
                Verdict::Pass,
                format!("{}ms", boot.reconcile_ms),
            ),
            boot.reconcile_ms as f64,
            "ms",
        ),
    ]
}

/// Per-producer ingest lag for the newest report of each, in one query.
async fn lags(pool: &MySqlPool) -> sqlx::Result<Vec<(String, String, i64, Option<u64>)>> {
    // ⚠ `interval_s` is BIGINT UNSIGNED, so it decodes as u64 and NOT i64 —
    // sqlx refuses the mismatch, and only against a real row, so a compile-clean
    // signed decode fails the first time it meets data.
    let rows: Vec<(String, String, i64, Option<u64>)> = sqlx::query_as(
        "SELECT r.source, r.collector, \
                TIMESTAMPDIFF(SECOND, r.collected_at, r.received_at), r.interval_s \
         FROM latest_report l JOIN report r ON r.id = l.report_id \
         ORDER BY r.source, r.collector",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Build this pod's self-report.
pub async fn build(
    pool: &MySqlPool,
    boot: Boot,
    sweep: Option<Sweep>,
) -> sqlx::Result<ReportUpload> {
    let started = std::time::Instant::now();
    let now = Utc::now();

    let mut checks = vec![pool_check(pool.size(), pool.num_idle())];
    for (source, collector, lag_s, interval_s) in lags(pool).await? {
        // Its own row would always read zero — it is written by this process at
        // the moment it is collected — and a zero nobody can act on is noise on
        // a page whose job is to be scanned.
        if source == SOURCE && collector == COLLECTOR {
            continue;
        }
        checks.push(lag_check(&source, &collector, lag_s, interval_s));
    }
    checks.push(retention_check(sweep, now));
    checks.extend(boot_checks(boot));

    Ok(ReportUpload {
        schema: crate::report::types::SCHEMA,
        id: Ulid::new().to_string(),
        collector: COLLECTOR.into(),
        collected_at: now,
        duration_ms: Some(started.elapsed().as_millis() as u64),
        interval_s: Some(INTERVAL.as_secs()),
        checks,
    })
}

/// Build and store one self-report. Ingests directly rather than POSTing to
/// itself: the HTTP round trip would add a token to configure and a failure mode
/// (its own listener) that has nothing to do with what is being measured.
pub async fn run_once(pool: &MySqlPool, boot: Boot, sweep: Option<Sweep>) -> anyhow::Result<()> {
    let upload = build(pool, boot, sweep).await?;
    let raw = serde_json::json!({ "note": "generated in-process by selfcheck" }).to_string();
    repo::ingest(pool, SOURCE, &upload, &raw).await?;
    Ok(())
}
