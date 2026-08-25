//! Pins the `/api/problems` wire shape to a committed golden fixture
//! (`tests/golden/problems.json`).
//!
//! The Angular client is bound to the Rust types by construction (ts-rs), but
//! the Android poller hand-parses the JSON with org.json — and it degrades
//! instead of throwing, so a renamed field would not crash it; it would quietly
//! stop seeing problems, which is the exact failure fleetwatch exists to catch.
//! The golden file makes that drift loud:
//!
//! - this test regenerates the fixture from the real Rust types — run it with
//!   `cargo test export_golden`; the gate runs it and then fails on
//!   `git diff --exit-code -- tests/golden` when the committed copy differs,
//! - the Android unit tests (`GoldenWireTest.kt`) parse the same file and
//!   assert on every field the poller consumes.
//!
//! Every `Option` appears once set and once absent, so both JSON shapes
//! (`"x": null` vs value) stay pinned.

use chrono::{DateTime, TimeZone, Utc};
use fleetwatch::report::types::{
    Freshness, Mute, MutedCheck, OverviewEntry, ProblemCheck, Problems, Verdict,
};

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 3, h, m, 0).unwrap()
}

#[test]
fn export_golden_problems() {
    let problems = Problems {
        checks: vec![
            // Every optional set.
            ProblemCheck {
                source: "mac-mini".into(),
                collector: "home-receivers".into(),
                report_id: "01JZFIXTURE0000000000000A".into(),
                section: "receivers".into(),
                label: "pixel5".into(),
                subject: Some("pixel5".into()),
                verdict: Verdict::Fail,
                observed: Some("last push 414 min ago".into()),
                expected: Some("< 30 min".into()),
                doc_ref: Some("receivers.md:12".into()),
                // Multi-line on purpose: `detail` is a captured gate log, and
                // the Android poller hand-parses this shape with org.json. A
                // single-line value would not exercise the escaping.
                detail: Some("memview gate — 15 checks\n  x frontend build\n  x eslint".into()),
                collected_at: at(14, 0),
                // A fault that has been standing for hours, so the poller's
                // parse of an age is exercised rather than only its null case.
                first_seen: Some(at(2, 0)),
            },
            // Every optional absent.
            ProblemCheck {
                source: "mac-mini".into(),
                collector: "fleet-health".into(),
                report_id: "01JZFIXTURE0000000000000B".into(),
                section: "NIXOS RELEASE PARITY".into(),
                label: "release parity".into(),
                subject: None,
                verdict: Verdict::Warn,
                observed: None,
                expected: None,
                doc_ref: None,
                detail: None,
                collected_at: at(14, 5),
                // Null on purpose: the "every optional absent" row. A run older
                // than the backfill reads this way, and the poller must handle
                // it rather than assuming an age is always there.
                first_seen: None,
            },
        ],
        muted: vec![MutedCheck {
            source: "mac-mini".into(),
            collector: "doc-checks".into(),
            report_id: "01JZFIXTURE0000000000000C".into(),
            section: "docs".into(),
            label: "backup drill age".into(),
            subject: None,
            verdict: Verdict::Fail,
            observed: Some("drill 40 days old".into()),
            doc_ref: None,
            detail: None,
            collected_at: at(14, 10),
            mute_id: "01JZFIXTURE0000000000000D".into(),
            reason: "drill scheduled for the weekend".into(),
            expires_at: at(20, 0),
        }],
        stale: vec![OverviewEntry {
            source: "isis".into(),
            collector: "backup-verify".into(),
            report_id: "01JZFIXTURE0000000000000E".into(),
            collected_at: at(2, 0),
            age_s: 43_200,
            interval_s: Some(21_600),
            freshness: Freshness::Silent,
            worst: Verdict::Pass,
            pass: 12,
            warn: 0,
            fail: 0,
            skip: 1,
            muted: 0,
            total: 13,
        }],
        // A mute in the last quarter of its life. The Android poller parses this
        // by hand with org.json, so the field is here to pin its SHAPE — an
        // expiry it can compare against now, and the identity it suppresses.
        lapsing: vec![Mute {
            id: "01JZFIXTURE0000000000000F".into(),
            source: "odin".into(),
            collector: "restic".into(),
            label: "restic drill".into(),
            reason: "drill scheduled for the weekend".into(),
            created_by: "pippijn".into(),
            created_at: at(1, 0),
            expires_at: at(20, 0),
        }],
        // A retired producer that started reporting again. Pinned here because
        // it is the one entry on this page that means the opposite of the rest:
        // everything else is something that stopped, this is something that
        // started. `freshness` is Fresh on purpose — a returned producer is by
        // definition current, which is why `stale` could never carry it.
        returned: vec![OverviewEntry {
            source: "amun".into(),
            collector: "picade-drift".into(),
            report_id: "01JZFIXTURE0000000000000G".into(),
            collected_at: at(14, 12),
            age_s: 60,
            interval_s: Some(3_600),
            freshness: Freshness::Fresh,
            worst: Verdict::Pass,
            pass: 5,
            warn: 0,
            fail: 0,
            skip: 0,
            muted: 0,
            total: 5,
        }],
    };

    let json = serde_json::to_string_pretty(&problems).unwrap() + "\n";
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/problems.json"), json).unwrap();
}
