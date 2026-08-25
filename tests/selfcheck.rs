//! Pure unit tests for fleetwatch's self-report — no DB, no clock.
//!
//! Every judgement this module makes lives in these four functions, so the
//! boundaries are pinned exactly. The point of a self-report is that somebody
//! reads it; a threshold that fires in the steady state is one nobody reads.

use chrono::{Duration, Utc};
use fleetwatch::db::MAX_CONNECTIONS;
use fleetwatch::report::types::Verdict;
use fleetwatch::selfcheck::{Boot, Sweep, boot_checks, lag_check, pool_check, retention_check};

#[test]
fn pool_bands() {
    // size = connections OPENED, idle = of those, unused. in_use = size - idle.
    // A quiet pool: 2 open, both idle.
    assert_eq!(pool_check(2, 2).verdict, Verdict::Pass);
    // 5 of 8 in use — 62%, below the 75% warning.
    assert_eq!(pool_check(5, 0).verdict, Verdict::Pass);
    // 6 of 8 is exactly 75%.
    assert_eq!(pool_check(6, 0).verdict, Verdict::Warn);
    // Exhausted: every connection the ceiling allows is checked out. This is
    // #1053, whose only symptom was 500s.
    assert_eq!(pool_check(MAX_CONNECTIONS, 0).verdict, Verdict::Fail);
}

#[test]
fn a_pool_that_has_never_been_under_load_is_not_pressure() {
    // The pool opens connections lazily, so an idle service reports a small
    // size. Reading that as "0 of 8" rather than as pressure is the honest
    // answer, and it is why the fraction is over MAX_CONNECTIONS and not over
    // whatever happens to be open.
    let c = pool_check(1, 1);
    assert_eq!(c.verdict, Verdict::Pass);
    assert_eq!(c.value, Some(0.0));
}

#[test]
fn lag_is_judged_against_the_producers_own_interval() {
    // Arrived within its collection period: the spool is keeping up.
    assert_eq!(
        lag_check("mac-mini", "x", 60, Some(3600)).verdict,
        Verdict::Pass
    );
    // Exactly one period is still fine — it drained in the time it had.
    assert_eq!(
        lag_check("mac-mini", "x", 3600, Some(3600)).verdict,
        Verdict::Pass
    );
    // Longer than a period: arriving slower than it collects, so the backlog
    // grows. True whatever the period is, which is the point of not choosing a
    // constant here.
    assert_eq!(
        lag_check("mac-mini", "x", 3601, Some(3600)).verdict,
        Verdict::Warn
    );
    // A one-minute producer is held to one minute, not to an hour.
    assert_eq!(
        lag_check("mac-mini", "x", 61, Some(60)).verdict,
        Verdict::Warn
    );
}

#[test]
fn lag_without_a_declared_interval_reports_and_judges_nothing() {
    // There is no self-calibrating threshold available, and inventing one would
    // be a magic constant wearing the producer's name.
    let c = lag_check("mac-mini", "x", 99_999, None);
    assert_eq!(c.verdict, Verdict::Pass);
    assert_eq!(c.value, Some(99_999.0));
    assert!(c.observed.unwrap().contains("no declared interval"));
}

#[test]
fn retention_says_skip_before_the_first_sweep_rather_than_pass() {
    // A pod that has just booted has swept nothing. Pass would be a claim it
    // cannot support; Skip says "not measured", which is what Skip is for.
    assert_eq!(retention_check(None, Utc::now()).verdict, Verdict::Skip);
}

#[test]
fn retention_bands() {
    let now = Utc::now();
    let sweep = |ago: Duration| {
        Some(Sweep {
            at: now - ago,
            raw_cleared: 477,
            checks_deleted: 0,
        })
    };
    // The sweeper ticks every 24h; an hour of jitter is allowed so the steady
    // state stays quiet.
    assert_eq!(
        retention_check(sweep(Duration::hours(1)), now).verdict,
        Verdict::Pass
    );
    assert_eq!(
        retention_check(sweep(Duration::hours(24)), now).verdict,
        Verdict::Pass
    );
    assert_eq!(
        retention_check(sweep(Duration::hours(25)), now).verdict,
        Verdict::Pass
    );
    // Past the allowance: the sweeper is named within one cycle rather than at
    // disk-full.
    assert_eq!(
        retention_check(sweep(Duration::hours(25) + Duration::seconds(1)), now).verdict,
        Verdict::Fail
    );
}

#[test]
fn boot_costs_are_reported_not_judged() {
    // #1069's complaint is that 47s of migration was INVISIBLE, not that it was
    // wrong. A threshold here would be inventing a policy nobody asked for.
    let checks = boot_checks(Boot {
        migrate_ms: 47_000,
        reconcile_ms: 1_112,
    });
    assert_eq!(checks.len(), 2);
    assert!(checks.iter().all(|c| c.verdict == Verdict::Pass));
    assert_eq!(checks[0].value, Some(47_000.0));
    assert_eq!(checks[1].value, Some(1_112.0));
}
