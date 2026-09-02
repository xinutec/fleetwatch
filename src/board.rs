//! Render `/api/problems` for a terminal — the CLI half of what the phone's
//! poller already decides (#1312).
//!
//! The phone's decisions are MIRRORED here, not re-derived: `count` and
//! `notifiable_count` are `Problems.kt`'s `count` and `notifiable()`, kept in
//! lockstep by `tests/board.rs` against the same golden fixture the Kotlin
//! parser is tested on. The phone's notification number is a snapshot of when
//! the set last CHANGED, so it can lag this output — but the two must never
//! disagree about what the set IS.

use std::fmt::Write as _;

use crate::report::types::{Problems, Verdict};

/// The board count, as the phone computes it: failing/warning checks plus
/// silent/overdue collectors plus retired producers that are reporting again.
/// Muted rows are deliberately outside it — a mute exists to keep the count
/// quiet while staying visible.
pub fn count(p: &Problems) -> usize {
    p.checks.len() + p.stale.len() + p.returned.len()
}

/// The subset the phone would wake for: `count` with WARN checks dropped.
/// A warning is something to know, not something to do — some are true
/// forever by design, and notifying on those trains you to swipe.
pub fn notifiable_count(p: &Problems) -> usize {
    p.checks
        .iter()
        .filter(|c| c.verdict != Verdict::Warn)
        .count()
        + p.stale.len()
        + p.returned.len()
}

fn age(seconds: i64) -> String {
    match seconds {
        s if s < 120 => format!("{s}s"),
        s if s < 7200 => format!("{}m", s / 60),
        s if s < 172_800 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// One line per row, sections only when non-empty. A clean board is one quiet
/// line — quiet means quiet, not empty headings.
pub fn render(p: &Problems) -> String {
    let mut out = String::new();
    let (n, notif) = (count(p), notifiable_count(p));
    if n == 0 {
        out.push_str("board clear — nothing failing, nothing silent\n");
    } else {
        let _ = writeln!(
            out,
            "{n} problem(s) on the board; the phone notifies on {notif} — \
             warnings stay on the dashboard"
        );
    }
    for c in &p.checks {
        let _ = write!(
            out,
            "  {:4}  {}/{}  {}/{}",
            c.verdict, c.source, c.collector, c.section, c.label
        );
        if let Some(obs) = &c.observed {
            let _ = write!(out, " — {obs}");
        }
        if let Some(exp) = &c.expected {
            let _ = write!(out, " (expected {exp})");
        }
        if let Some(since) = &c.first_seen {
            let _ = write!(out, "  [since {}]", since.format("%Y-%m-%d %H:%M"));
        }
        out.push('\n');
    }
    for s in &p.stale {
        let _ = writeln!(
            out,
            "  stale  {}/{} — {}, last report {} ago",
            s.source,
            s.collector,
            format!("{:?}", s.freshness).to_lowercase(),
            age(s.age_s),
        );
    }
    for r in &p.returned {
        let _ = writeln!(
            out,
            "  returned  {}/{} — retired, but a report arrived {} ago",
            r.source,
            r.collector,
            age(r.age_s),
        );
    }
    if !p.muted.is_empty() {
        let _ = writeln!(out, "muted ({}, not counted):", p.muted.len());
        for m in &p.muted {
            let _ = writeln!(
                out,
                "  {:4}  {}/{}  {}/{} — {} (expires {})",
                m.verdict,
                m.source,
                m.collector,
                m.section,
                m.label,
                m.reason,
                m.expires_at.format("%Y-%m-%d %H:%M"),
            );
        }
    }
    if !p.lapsing.is_empty() {
        let _ = writeln!(out, "lapsing mutes ({}):", p.lapsing.len());
        for m in &p.lapsing {
            let _ = writeln!(
                out,
                "  {}/{}/{} — {} (expires {})",
                m.source,
                m.collector,
                m.label,
                m.reason,
                m.expires_at.format("%Y-%m-%d %H:%M"),
            );
        }
    }
    out
}
