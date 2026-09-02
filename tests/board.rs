//! The board formatter, tested against the same golden fixture that pins the
//! wire for Android (`tests/golden/problems.json`) — so the CLI, the phone and
//! the server cannot drift apart without one of these files saying so.

use fleetwatch::board;
use fleetwatch::report::types::Problems;

fn fixture() -> Problems {
    serde_json::from_str(include_str!("golden/problems.json"))
        .expect("the golden problems fixture deserializes into the wire types")
}

/// The response types were Serialize-only until the CLI needed to read them
/// back. This pins the round-trip on the same bytes the Kotlin parser is
/// tested on.
#[test]
fn the_wire_shape_deserializes() {
    let p = fixture();
    assert_eq!(p.checks.len(), 2);
    assert_eq!(p.muted.len(), 1);
    assert_eq!(p.stale.len(), 1);
    assert_eq!(p.lapsing.len(), 1);
    assert_eq!(p.returned.len(), 1);
}

/// The two numbers the CLI prints are the PHONE's numbers (Problems.kt):
/// `count` = checks + stale + returned, `notifiable` additionally drops WARN
/// from checks. A CLI that derived its own count would read as one of the two
/// being broken (#1312).
#[test]
fn counts_match_the_phone() {
    let p = fixture();
    assert_eq!(board::count(&p), 4);
    assert_eq!(board::notifiable_count(&p), 3);
}

#[test]
fn render_names_every_row_and_says_why_the_two_counts_differ() {
    let p = fixture();
    let out = board::render(&p);
    // Every section's row appears with its identity.
    assert!(out.contains("home-receivers"), "problem check row:\n{out}");
    assert!(out.contains("pixel5"), "problem check label:\n{out}");
    assert!(out.contains("muted"), "muted section:\n{out}");
    assert!(out.contains("stale"), "stale section:\n{out}");
    assert!(out.contains("returned"), "returned section:\n{out}");
    assert!(out.contains("lapsing"), "lapsing-mutes section:\n{out}");
    // Both counts, and the reason they differ, in one line a reader meets first.
    assert!(out.contains("4 problem"), "board count:\n{out}");
    assert!(out.contains("notifies on 3"), "phone count:\n{out}");
    assert!(out.to_lowercase().contains("warn"), "the why:\n{out}");
}

#[test]
fn a_clean_board_renders_quiet_and_counts_zero() {
    let p = Problems {
        checks: vec![],
        muted: vec![],
        stale: vec![],
        lapsing: vec![],
        returned: vec![],
    };
    assert_eq!(board::count(&p), 0);
    assert_eq!(board::notifiable_count(&p), 0);
    let out = board::render(&p);
    assert!(out.contains("clear"), "a clean board says so:\n{out}");
    // No empty section headings on a clean board — quiet means quiet.
    assert!(!out.contains("muted"), "no muted heading when none:\n{out}");
}
