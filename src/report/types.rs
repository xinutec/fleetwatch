//! Report + check types: the wire format producers POST, and the shapes the UI
//! reads back. Response types derive `TS` so `scripts/gen-types.sh` keeps the
//! Angular interfaces in lock-step (see frontend/src/app/generated/).
//!
//! Deliberate shape decisions (see docs/design.md §4.1):
//! - `source` is NOT in the upload — it's derived from the ingest token, so a
//!   producer can only write as itself.
//! - a check's trend identity is `(source, collector, section, label)`; `label`
//!   must be stable across runs, with run-varying data in `observed`/`value`.
//! - one optional numeric per check (`value`/`unit`) drives the trend charts.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The wire schema version fleetwatch currently accepts. Bumped only on a
/// breaking change to the report shape; the server rejects anything else (422).
pub const SCHEMA: u32 = 1;

/// A check's outcome. Mirrors the `Verdict` enum the CLI tools already use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Verdict {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Verdict::Pass => "pass",
            Verdict::Warn => "warn",
            Verdict::Fail => "fail",
            Verdict::Skip => "skip",
        })
    }
}

impl FromStr for Verdict {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pass" => Ok(Verdict::Pass),
            "warn" => Ok(Verdict::Warn),
            "fail" => Ok(Verdict::Fail),
            "skip" => Ok(Verdict::Skip),
            other => Err(format!("unknown verdict {other:?}")),
        }
    }
}

/// How current a collector's latest report is, computed at read time from the
/// report's declared `interval_s` (see `report::staleness`). A push-based
/// monitor's worst failure is a dead producer looking green, so this is
/// first-class: `Silent` renders as a failure, `Overdue` as a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Freshness {
    Fresh,
    Overdue,
    Silent,
}

/// The identity a check trends under (docs/design.md §4.1). One struct rather
/// than four positional strings, so a call site can't transpose collector and
/// section and still compile — a swap would just return an empty series.
#[derive(Debug, Clone)]
pub struct CheckKey {
    pub source: String,
    pub collector: String,
    pub section: String,
    pub label: String,
}

// --- upload (producer → fleetwatch). Deserialize only; producers aren't TS. ---

/// One report POSTed by a producer. `id` is a producer-minted ULID used as the
/// idempotency key (the spool may re-send after a network flap).
#[derive(Debug, Clone, Deserialize)]
pub struct ReportUpload {
    pub schema: u32,
    pub id: String,
    pub collector: String,
    pub collected_at: DateTime<Utc>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub interval_s: Option<u64>,
    pub checks: Vec<CheckUpload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckUpload {
    pub section: String,
    pub label: String,
    #[serde(default)]
    pub subject: Option<String>,
    pub verdict: Verdict,
    #[serde(default)]
    pub observed: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default, rename = "ref")]
    pub doc_ref: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

// --- responses (fleetwatch → UI). Serialize + TS. ---

/// One tile on the overview: a (source, collector) with its latest rollup.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OverviewEntry {
    pub source: String,
    pub collector: String,
    pub report_id: String,
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    #[ts(type = "number")]
    pub age_s: i64,
    #[ts(type = "number | null")]
    pub interval_s: Option<u64>,
    pub freshness: Freshness,
    /// Worst verdict among the report's *unmuted* checks (drives the tile colour):
    /// a tile whose only failure is muted is not red.
    pub worst: Verdict,
    pub pass: u32,
    /// `warn`/`fail` are the *live* counts — muted checks are subtracted out and
    /// surfaced in `muted` instead, so a green tile never also shows a red pill.
    /// The true stored counts remain in the report itself.
    pub warn: u32,
    pub fail: u32,
    pub skip: u32,
    /// Fail/warn checks currently suppressed by a live mute.
    pub muted: u32,
    /// Every check in the report, muted or not (`pass + warn + fail + skip + muted`).
    pub total: u32,
}

/// A single failing/warning check surfaced on the problems view.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProblemCheck {
    pub source: String,
    pub collector: String,
    pub report_id: String,
    pub section: String,
    pub label: String,
    pub subject: Option<String>,
    pub verdict: Verdict,
    pub observed: Option<String>,
    pub expected: Option<String>,
    #[serde(rename = "ref")]
    pub doc_ref: Option<String>,
    // ⚠ **A `///` here lands in frontend/src/app/generated/ProblemCheck.ts** —
    // ts-rs copies doc comments into what it emits, so the rationale below would
    // become twenty lines of incident history inside a generated wire type. The
    // one-line contract is the doc comment; the reasons are these `//` ones.
    //
    // **This field is what makes a red gate attributable.** `observed` is a
    // summary — for a gate row it is `"memview gate — 15 checks"`, a count with
    // no identity — so triage from this endpoint could say a repo's gate was red
    // and not which check inside it failed (#861).
    //
    // ⚠ **Returning it here was NOT sufficient, and the column alone is the
    // trap.** Measured 2026-08-18 across the whole `check_result` table, the
    // `verify` collector had populated `detail` ZERO times ever — so this field
    // read null for exactly the rows it was added for until dev-lint's producer
    // (`fleet.py`'s `_detail`) started sending it. A consumer change for data
    // nobody produces looks identical to a fix.
    //
    // It costs the response almost nothing: on the live problems view that day,
    // 56 of 57 rows had no detail at all. And it does not widen what the read
    // token reaches — the same latest-report fail/warn rows, not history.
    /// The producer's captured log for a failing run, when it sent one.
    pub detail: Option<String>,
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    // When this identity STARTED failing — the producer's clock at the first
    // report of the current run, maintained on ingest (migration 0006).
    //
    // Without it a check red for a week reads exactly like one red for a minute,
    // and the two want different responses. On 2026-08-21 claude-disk's trough
    // check had been failing eight days, naming in its own expected text the
    // mechanism that then broke a running test — and the diagnosis was rebuilt
    // from scratch because nothing here said "this is not new".
    //
    // ⚠ Null when unknown — a run older than the backfill, or one no report has
    // opened. NOT defaulted to now: "we do not know how long" and "it started
    // this second" are different claims and only one of them is true.
    /// When this check started failing, if known.
    #[ts(type = "string | null")]
    pub first_seen: Option<DateTime<Utc>>,
}

/// A failing/warning check that a live mute is currently suppressing. Shown in
/// its own section so a deliberate silence stays visible (not vanished), with
/// the reason and when it lapses.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MutedCheck {
    pub source: String,
    pub collector: String,
    pub report_id: String,
    pub section: String,
    pub label: String,
    pub subject: Option<String>,
    pub verdict: Verdict,
    pub observed: Option<String>,
    #[serde(rename = "ref")]
    pub doc_ref: Option<String>,
    // A muted row is still one somebody triages: deciding whether the mute is
    // still the right call needs the same evidence. See `ProblemCheck::detail`.
    /// The producer's captured log for a failing run, when it sent one.
    pub detail: Option<String>,
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    pub mute_id: String,
    pub reason: String,
    #[ts(type = "string")]
    pub expires_at: DateTime<Utc>,
}

/// The problems view: what's wrong right now — failing/warning checks, the ones
/// a live mute is suppressing (kept visible, not notified), plus collectors that
/// have gone silent/overdue (which no check can express).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Problems {
    pub checks: Vec<ProblemCheck>,
    pub muted: Vec<MutedCheck>,
    pub stale: Vec<OverviewEntry>,
    // Mutes about to run out. Synthesised here for the same reason `stale` is:
    // no producer can emit it. A mute lapsing is a fact about this service's own
    // state, and it is the ONE thing here that can be announced BEFORE it
    // happens rather than after.
    //
    // The picades' mutes lapsed on 2026-08-19 and their failures reappeared with
    // nothing said; the dashboard sat red for two days until Pippijn asked why.
    // That silence is correct for the expiry itself — a mute must expire, and no
    // reticketing is the point — but the expiry was knowable a week ahead.
    pub lapsing: Vec<Mute>,
}

/// A live suppression of one check identity `(source, collector, label)`. Always
/// has a reason and a hard expiry — see migrations/0003_mutes.sql.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Mute {
    pub id: String,
    pub source: String,
    pub collector: String,
    pub label: String,
    pub reason: String,
    pub created_by: String,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "string")]
    pub expires_at: DateTime<Utc>,
}

/// Request body to create a mute. The server stamps `created_by` from the
/// session and computes `expires_at` from `ttl_hours` (a mute cannot be
/// permanent), so neither is client-supplied.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NewMute {
    pub source: String,
    pub collector: String,
    pub label: String,
    pub reason: String,
    pub ttl_hours: u32,
}

/// A check as rendered in a report's detail view.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CheckOut {
    pub section: String,
    pub label: String,
    pub subject: Option<String>,
    pub verdict: Verdict,
    pub observed: Option<String>,
    pub expected: Option<String>,
    #[ts(type = "number | null")]
    pub value: Option<f64>,
    pub unit: Option<String>,
    #[serde(rename = "ref")]
    pub doc_ref: Option<String>,
    pub detail: Option<String>,
}

/// A full report with all its checks, grouped by the UI into sections.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReportDetail {
    pub id: String,
    pub source: String,
    pub collector: String,
    pub schema: u32,
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    #[ts(type = "string")]
    pub received_at: DateTime<Utc>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub interval_s: Option<u64>,
    pub ok: bool,
    pub checks: Vec<CheckOut>,
}

/// A row in the "runs" list (report history for one collector).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReportSummary {
    pub id: String,
    pub source: String,
    pub collector: String,
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    pub ok: bool,
    pub pass: u32,
    pub warn: u32,
    pub fail: u32,
    pub skip: u32,
    pub total: u32,
}

/// One point in a single check's time series.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HistoryPoint {
    #[ts(type = "string")]
    pub collected_at: DateTime<Utc>,
    pub verdict: Verdict,
    #[ts(type = "number | null")]
    pub value: Option<f64>,
}

/// The time series for one `(source, collector, section, label)` check.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct History {
    pub source: String,
    pub collector: String,
    pub section: String,
    pub label: String,
    pub unit: Option<String>,
    pub points: Vec<HistoryPoint>,
}

/// Response to a successful ingest.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IngestAck {
    pub id: String,
    /// true if this id was already stored (idempotent replay), false if new.
    pub duplicate: bool,
    pub checks: u32,
}
