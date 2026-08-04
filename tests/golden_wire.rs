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
    Freshness, MutedCheck, OverviewEntry, ProblemCheck, Problems, Verdict,
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
                collected_at: at(14, 0),
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
                collected_at: at(14, 5),
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
    };

    let json = serde_json::to_string_pretty(&problems).unwrap() + "\n";
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/problems.json"), json).unwrap();
}
