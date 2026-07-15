//! Persistence for reports + checks: ingest (idempotent) and the read queries
//! that back every UI view.
//!
//! All timestamps are handled as UTC. Columns are DATETIME(3); we bind
//! `naive_utc()` and read `NaiveDateTime` then `.and_utc()`, so nothing depends
//! on the DB session timezone. Ages are computed in Rust against `Utc::now()`.

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use sqlx::MySqlPool;
use ulid::Ulid;

use super::staleness::freshness;
use super::types::{
    CheckOut, CheckUpload, Freshness, History, HistoryPoint, IngestAck, Mute, MutedCheck, NewMute,
    OverviewEntry, ProblemCheck, Problems, ReportDetail, ReportSummary, ReportUpload, SCHEMA,
    Verdict,
};
use crate::error::AppError;

/// A mute's time-to-live is clamped to this range: at least an hour (a shorter
/// mute isn't worth recording) and at most 90 days (nothing stays silenced for a
/// season without a human re-deciding).
const MUTE_TTL_HOURS: std::ops::RangeInclusive<u32> = 1..=24 * 90;

/// Validate + store one uploaded report under `source` (from the token). Returns
/// an ack marking whether this was a fresh store or an idempotent replay.
///
/// `raw` is the exact request body, kept for schema-evolution replay. Validation
/// failures surface as `AppError::Unprocessable` (422); the caller has already
/// authenticated.
pub async fn ingest(
    pool: &MySqlPool,
    source: &str,
    upload: &ReportUpload,
    raw: &str,
) -> Result<IngestAck, AppError> {
    if upload.schema != SCHEMA {
        return Err(AppError::Unprocessable(format!(
            "unsupported schema version {} (this fleetwatch accepts {SCHEMA})",
            upload.schema
        )));
    }
    // The id is the idempotency key + primary key — it must be a real ULID.
    if Ulid::from_string(&upload.id).is_err() {
        return Err(AppError::Unprocessable(format!(
            "id {:?} is not a valid ULID",
            upload.id
        )));
    }
    if upload.collector.trim().is_empty() {
        return Err(AppError::Unprocessable(
            "collector must not be empty".into(),
        ));
    }

    let (mut n_pass, mut n_warn, mut n_fail, mut n_skip) = (0u32, 0u32, 0u32, 0u32);
    for c in &upload.checks {
        match c.verdict {
            Verdict::Pass => n_pass += 1,
            Verdict::Warn => n_warn += 1,
            Verdict::Fail => n_fail += 1,
            Verdict::Skip => n_skip += 1,
        }
    }
    let ok = n_fail == 0;
    let collected = upload.collected_at.naive_utc();
    let received = Utc::now().naive_utc();

    let mut tx = pool.begin().await.map_err(AppError::from)?;

    // INSERT IGNORE: a duplicate id (spool replay) leaves rows_affected == 0, so
    // we skip re-inserting the checks and report it as a no-op. The FK from
    // check_result means the checks only exist if the report row was created.
    let res = sqlx::query(
        "INSERT IGNORE INTO report \
         (id, source, collector, schema_ver, collected_at, received_at, duration_ms, \
          interval_s, ok, n_pass, n_warn, n_fail, n_skip, raw) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&upload.id)
    .bind(source)
    .bind(&upload.collector)
    .bind(upload.schema)
    .bind(collected)
    .bind(received)
    .bind(upload.duration_ms)
    .bind(upload.interval_s)
    .bind(ok)
    .bind(n_pass)
    .bind(n_warn)
    .bind(n_fail)
    .bind(n_skip)
    .bind(raw)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    if res.rows_affected() == 0 {
        tx.rollback().await.map_err(AppError::from)?;
        return Ok(IngestAck {
            id: upload.id.clone(),
            duplicate: true,
            checks: 0,
        });
    }

    for (seq, c) in upload.checks.iter().enumerate() {
        insert_check(
            &mut tx,
            &upload.id,
            source,
            &upload.collector,
            collected,
            seq as u32,
            c,
        )
        .await?;
    }
    tx.commit().await.map_err(AppError::from)?;

    Ok(IngestAck {
        id: upload.id.clone(),
        duplicate: false,
        checks: upload.checks.len() as u32,
    })
}

async fn insert_check(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    report_id: &str,
    source: &str,
    collector: &str,
    collected: NaiveDateTime,
    seq: u32,
    c: &CheckUpload,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO check_result \
         (report_id, seq, source, collector, collected_at, section, label, subject, \
          verdict, observed, expected, value, unit, doc_ref, detail) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(report_id)
    .bind(seq)
    .bind(source)
    .bind(collector)
    .bind(collected)
    .bind(&c.section)
    .bind(&c.label)
    .bind(&c.subject)
    .bind(c.verdict.to_string())
    .bind(&c.observed)
    .bind(&c.expected)
    .bind(c.value)
    .bind(&c.unit)
    .bind(&c.doc_ref)
    .bind(&c.detail)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

fn parse_verdict(s: &str) -> Result<Verdict> {
    s.parse::<Verdict>().map_err(anyhow::Error::msg)
}

fn worst_of(n_fail: u32, n_warn: u32, n_pass: u32) -> Verdict {
    if n_fail > 0 {
        Verdict::Fail
    } else if n_warn > 0 {
        Verdict::Warn
    } else if n_pass > 0 {
        Verdict::Pass
    } else {
        Verdict::Skip
    }
}

#[derive(sqlx::FromRow)]
struct LatestRow {
    id: String,
    source: String,
    collector: String,
    collected_at: NaiveDateTime,
    interval_s: Option<u64>,
    n_pass: u32,
    n_warn: u32,
    n_fail: u32,
    n_skip: u32,
    /// Fail/warn checks in this report whose identity has a live mute. Computed
    /// by the query (0 when nothing is muted) — the tile discounts these so a
    /// deliberately-silenced fault does not keep it red.
    muted_fail: u32,
    muted_warn: u32,
}

/// Latest report per (source, collector) → one overview tile each, newest first
/// by source/collector name. Freshness is computed here against the wall clock.
///
/// The LEFT JOIN counts, per latest report, how many of its fail/warn checks are
/// covered by a live mute; the tile's `worst` is computed against the unmuted
/// remainder so a muted-only failure shows green (with a "muted" marker), while
/// the raw `fail`/`warn` counts stay the true stored numbers.
pub async fn overview(pool: &MySqlPool) -> Result<Vec<OverviewEntry>> {
    let rows: Vec<LatestRow> = sqlx::query_as(
        // SUM over a boolean is DECIMAL in MariaDB; CAST back to UNSIGNED so the
        // column decodes into u32.
        "SELECT x.id, x.source, x.collector, x.collected_at, x.interval_s, \
                x.n_pass, x.n_warn, x.n_fail, x.n_skip, \
                CAST(COALESCE(m.muted_fail, 0) AS UNSIGNED) AS muted_fail, \
                CAST(COALESCE(m.muted_warn, 0) AS UNSIGNED) AS muted_warn \
         FROM ( \
           SELECT r.*, ROW_NUMBER() OVER \
             (PARTITION BY source, collector ORDER BY collected_at DESC, id DESC) rn \
           FROM report r \
         ) x \
         LEFT JOIN ( \
           SELECT c.report_id, \
                  SUM(c.verdict = 'fail') AS muted_fail, \
                  SUM(c.verdict = 'warn') AS muted_warn \
           FROM check_result c \
           JOIN mute mu ON mu.source = c.source AND mu.collector = c.collector \
                       AND mu.label = c.label AND mu.expires_at > ? \
           WHERE c.verdict IN ('fail', 'warn') \
           GROUP BY c.report_id \
         ) m ON m.report_id = x.id \
         WHERE x.rn = 1 ORDER BY x.source, x.collector",
    )
    // Bind UTC now rather than SQL NOW(3): the DB server clock is local, and the
    // stored timestamps are UTC — comparing them in SQL would offset the mute
    // window by the server's timezone. (Same invariant as the rest of this file.)
    .bind(Utc::now().naive_utc())
    .fetch_all(pool)
    .await?;

    let now = Utc::now();
    Ok(rows.into_iter().map(|r| overview_entry(&now, r)).collect())
}

fn overview_entry(now: &DateTime<Utc>, r: LatestRow) -> OverviewEntry {
    let collected = r.collected_at.and_utc();
    let age_s = (*now - collected).num_seconds();
    // Carve muted fail/warn out of the live counts: the tile colour and its
    // warn/fail pills reflect only unmuted checks, with the muted ones shown
    // separately. The report's own stored counts stay the untouched truth.
    let live_fail = r.n_fail.saturating_sub(r.muted_fail);
    let live_warn = r.n_warn.saturating_sub(r.muted_warn);
    OverviewEntry {
        source: r.source,
        collector: r.collector,
        report_id: r.id,
        collected_at: collected,
        age_s,
        interval_s: r.interval_s,
        freshness: freshness(age_s, r.interval_s),
        worst: worst_of(live_fail, live_warn, r.n_pass),
        pass: r.n_pass,
        warn: live_warn,
        fail: live_fail,
        skip: r.n_skip,
        muted: r.muted_fail + r.muted_warn,
        total: r.n_pass + r.n_warn + r.n_fail + r.n_skip,
    }
}

#[derive(sqlx::FromRow)]
struct ProblemRow {
    source: String,
    collector: String,
    report_id: String,
    section: String,
    label: String,
    subject: Option<String>,
    verdict: String,
    observed: Option<String>,
    expected: Option<String>,
    doc_ref: Option<String>,
    collected_at: NaiveDateTime,
}

/// The problems view: every failing/warning check from each collector's latest
/// report, plus collectors whose latest report has gone overdue/silent (which no
/// check can express — a dead producer emits nothing).
///
/// A check whose identity has a live mute is moved out of `checks` (so the
/// notifier stays quiet) into `muted` (kept visible, with reason + expiry). The
/// mute is matched in Rust against the active-mute set rather than a SQL join, so
/// two overlapping mutes on one identity can't duplicate a row.
pub async fn problems(pool: &MySqlPool) -> Result<Problems> {
    let rows: Vec<ProblemRow> = sqlx::query_as(
        "WITH latest AS ( \
           SELECT id FROM ( \
             SELECT id, ROW_NUMBER() OVER \
               (PARTITION BY source, collector ORDER BY collected_at DESC, id DESC) rn \
             FROM report \
           ) x WHERE rn = 1 \
         ) \
         SELECT c.source, c.collector, c.report_id, c.section, c.label, c.subject, \
                c.verdict, c.observed, c.expected, c.doc_ref, c.collected_at \
         FROM check_result c JOIN latest l ON c.report_id = l.id \
         WHERE c.verdict IN ('fail', 'warn') \
         ORDER BY FIELD(c.verdict, 'fail', 'warn'), c.source, c.collector, c.section, c.seq",
    )
    .fetch_all(pool)
    .await?;

    // The active-mute set, keyed by identity → the mute that expires latest.
    let mutes = list_mutes(pool).await?;
    let mut by_id: std::collections::HashMap<(&str, &str, &str), &Mute> =
        std::collections::HashMap::new();
    for m in &mutes {
        by_id
            .entry((&m.source, &m.collector, &m.label))
            .and_modify(|cur| {
                if m.expires_at > cur.expires_at {
                    *cur = m;
                }
            })
            .or_insert(m);
    }

    let mut checks = Vec::new();
    let mut muted = Vec::new();
    for r in &rows {
        let verdict = parse_verdict(&r.verdict)?;
        match by_id.get(&(r.source.as_str(), r.collector.as_str(), r.label.as_str())) {
            Some(m) => muted.push(MutedCheck {
                source: r.source.clone(),
                collector: r.collector.clone(),
                report_id: r.report_id.clone(),
                section: r.section.clone(),
                label: r.label.clone(),
                subject: r.subject.clone(),
                verdict,
                observed: r.observed.clone(),
                doc_ref: r.doc_ref.clone(),
                collected_at: r.collected_at.and_utc(),
                mute_id: m.id.clone(),
                reason: m.reason.clone(),
                expires_at: m.expires_at,
            }),
            None => checks.push(ProblemCheck {
                source: r.source.clone(),
                collector: r.collector.clone(),
                report_id: r.report_id.clone(),
                section: r.section.clone(),
                label: r.label.clone(),
                subject: r.subject.clone(),
                verdict,
                observed: r.observed.clone(),
                expected: r.expected.clone(),
                doc_ref: r.doc_ref.clone(),
                collected_at: r.collected_at.and_utc(),
            }),
        }
    }

    let stale = overview(pool)
        .await?
        .into_iter()
        .filter(|e| e.freshness != Freshness::Fresh)
        .collect();

    Ok(Problems {
        checks,
        muted,
        stale,
    })
}

/// Create a mute. `created_by` comes from the session; `expires_at` is derived
/// from `ttl_hours` (clamped) — a mute cannot be permanent. Identity fields and
/// the reason must be non-empty: an unattributable or unbounded mute is exactly
/// the silent blind spot this feature exists to avoid.
pub async fn create_mute(
    pool: &MySqlPool,
    new: &NewMute,
    created_by: &str,
) -> Result<Mute, AppError> {
    let source = new.source.trim();
    let collector = new.collector.trim();
    let label = new.label.trim();
    let reason = new.reason.trim();
    if source.is_empty() || collector.is_empty() || label.is_empty() {
        return Err(AppError::BadRequest(
            "source, collector and label are required".into(),
        ));
    }
    if reason.is_empty() {
        return Err(AppError::BadRequest("a reason is required".into()));
    }
    let ttl = new
        .ttl_hours
        .clamp(*MUTE_TTL_HOURS.start(), *MUTE_TTL_HOURS.end());

    let id = Ulid::new().to_string();
    let created_at = Utc::now();
    let expires_at = created_at + Duration::hours(ttl as i64);

    sqlx::query(
        "INSERT INTO mute (id, source, collector, label, reason, created_by, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(source)
    .bind(collector)
    .bind(label)
    .bind(reason)
    .bind(created_by)
    .bind(created_at.naive_utc())
    .bind(expires_at.naive_utc())
    .execute(pool)
    .await
    .map_err(AppError::from)?;

    Ok(Mute {
        id,
        source: source.to_string(),
        collector: collector.to_string(),
        label: label.to_string(),
        reason: reason.to_string(),
        created_by: created_by.to_string(),
        created_at,
        expires_at,
    })
}

#[derive(sqlx::FromRow)]
struct MuteRow {
    id: String,
    source: String,
    collector: String,
    label: String,
    reason: String,
    created_by: String,
    created_at: NaiveDateTime,
    expires_at: NaiveDateTime,
}

/// Every mute that is still live (not yet expired), newest first. Lapsed mutes
/// stay in the table as an audit trail but are never returned or applied.
pub async fn list_mutes(pool: &MySqlPool) -> Result<Vec<Mute>> {
    let rows: Vec<MuteRow> = sqlx::query_as(
        "SELECT id, source, collector, label, reason, created_by, created_at, expires_at \
         FROM mute WHERE expires_at > ? ORDER BY created_at DESC",
    )
    // UTC now, not SQL NOW(3) — see the note in `overview`.
    .bind(Utc::now().naive_utc())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Mute {
            id: r.id,
            source: r.source,
            collector: r.collector,
            label: r.label,
            reason: r.reason,
            created_by: r.created_by,
            created_at: r.created_at.and_utc(),
            expires_at: r.expires_at.and_utc(),
        })
        .collect())
}

/// Delete a mute early (unmute). Returns whether a row existed. The suppressed
/// problem reappears on the next read immediately — mutes are read-time overlays.
pub async fn delete_mute(pool: &MySqlPool, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM mute WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: String,
    source: String,
    collector: String,
    collected_at: NaiveDateTime,
    duration_ms: Option<u64>,
    ok: bool,
    n_pass: u32,
    n_warn: u32,
    n_fail: u32,
    n_skip: u32,
}

/// Report history (the "runs" list), newest first, optionally filtered by
/// source and/or collector. `limit` is clamped by the caller.
pub async fn list_reports(
    pool: &MySqlPool,
    source: Option<&str>,
    collector: Option<&str>,
    limit: u32,
) -> Result<Vec<ReportSummary>> {
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT id, source, collector, collected_at, duration_ms, ok, \
                n_pass, n_warn, n_fail, n_skip \
         FROM report \
         WHERE (? IS NULL OR source = ?) AND (? IS NULL OR collector = ?) \
         ORDER BY collected_at DESC, id DESC LIMIT ?",
    )
    .bind(source)
    .bind(source)
    .bind(collector)
    .bind(collector)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ReportSummary {
            id: r.id,
            source: r.source,
            collector: r.collector,
            collected_at: r.collected_at.and_utc(),
            duration_ms: r.duration_ms,
            ok: r.ok,
            pass: r.n_pass,
            warn: r.n_warn,
            fail: r.n_fail,
            skip: r.n_skip,
            total: r.n_pass + r.n_warn + r.n_fail + r.n_skip,
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct DetailRow {
    id: String,
    source: String,
    collector: String,
    schema_ver: u32,
    collected_at: NaiveDateTime,
    received_at: NaiveDateTime,
    duration_ms: Option<u64>,
    interval_s: Option<u64>,
    ok: bool,
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    section: String,
    label: String,
    subject: Option<String>,
    verdict: String,
    observed: Option<String>,
    expected: Option<String>,
    value: Option<f64>,
    unit: Option<String>,
    doc_ref: Option<String>,
    detail: Option<String>,
}

/// One report with all its checks in report order, or None if the id is unknown.
pub async fn report_detail(pool: &MySqlPool, id: &str) -> Result<Option<ReportDetail>> {
    let Some(r): Option<DetailRow> = sqlx::query_as(
        "SELECT id, source, collector, schema_ver, collected_at, received_at, \
                duration_ms, interval_s, ok FROM report WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let check_rows: Vec<CheckRow> = sqlx::query_as(
        "SELECT section, label, subject, verdict, observed, expected, value, unit, \
                doc_ref, detail FROM check_result WHERE report_id = ? ORDER BY seq",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let checks = check_rows
        .into_iter()
        .map(|c| -> Result<CheckOut> {
            Ok(CheckOut {
                section: c.section,
                label: c.label,
                subject: c.subject,
                verdict: parse_verdict(&c.verdict)?,
                observed: c.observed,
                expected: c.expected,
                value: c.value,
                unit: c.unit,
                doc_ref: c.doc_ref,
                detail: c.detail,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Some(ReportDetail {
        id: r.id,
        source: r.source,
        collector: r.collector,
        schema: r.schema_ver,
        collected_at: r.collected_at.and_utc(),
        received_at: r.received_at.and_utc(),
        duration_ms: r.duration_ms,
        interval_s: r.interval_s,
        ok: r.ok,
        checks,
    }))
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    collected_at: NaiveDateTime,
    verdict: String,
    value: Option<f64>,
    unit: Option<String>,
}

/// Time series for one `(source, collector, section, label)` check between two
/// instants, oldest first. `unit` is taken from the most recent point that has
/// one (the current meaning of the numeric).
pub async fn history(
    pool: &MySqlPool,
    source: &str,
    collector: &str,
    section: &str,
    label: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<History> {
    let rows: Vec<HistoryRow> = sqlx::query_as(
        "SELECT collected_at, verdict, value, unit FROM check_result \
         WHERE source = ? AND collector = ? AND section = ? AND label = ? \
           AND collected_at BETWEEN ? AND ? \
         ORDER BY collected_at",
    )
    .bind(source)
    .bind(collector)
    .bind(section)
    .bind(label)
    .bind(from.naive_utc())
    .bind(to.naive_utc())
    .fetch_all(pool)
    .await?;

    let unit = rows.iter().rev().find_map(|r| r.unit.clone());
    let points = rows
        .into_iter()
        .map(|r| -> Result<HistoryPoint> {
            Ok(HistoryPoint {
                collected_at: r.collected_at.and_utc(),
                verdict: parse_verdict(&r.verdict)?,
                value: r.value,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(History {
        source: source.to_string(),
        collector: collector.to_string(),
        section: section.to_string(),
        label: label.to_string(),
        unit,
        points,
    })
}
