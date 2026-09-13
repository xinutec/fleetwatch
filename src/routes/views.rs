//! The read side: GET endpoints backing every UI view. Most require a valid
//! Nextcloud-login session (the `AuthUser` extractor 401s otherwise) — the VPN
//! is no longer the gate. `/api/problems` and `/api/history` take `Reader`
//! instead, which accepts that session OR a read token; both are keyed or
//! summary answers that enumerate nothing. Each handler is thin and delegates to
//! `report::repo`.

use axum::Json;
use axum::extract::{Path, Query, State};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::error::AppError;
use crate::report::repo;
use crate::report::types::{
    CheckKey, History, OverviewEntry, Problems, ReportDetail, ReportSummary,
};
use crate::session::{AuthUser, Reader};
use crate::state::AppState;

/// GET /api/overview — one tile per (source, collector) with its latest rollup.
pub async fn overview(
    _user: AuthUser,
    State(app): State<AppState>,
) -> Result<Json<Vec<OverviewEntry>>, AppError> {
    Ok(Json(repo::overview(&app.pool).await?))
}

/// GET /api/problems — failing/warning checks + overdue/silent collectors.
///
/// Reachable by a *read token* (`Reader`, not `AuthUser`): the Android app polls it
/// from the background to decide whether to raise a notification, and a background
/// worker can't complete an interactive Nextcloud login.
///
/// It was the ONLY such endpoint until 2026-09-13; `/api/history` is the second.
/// The rest stay session-only.
pub async fn problems(
    _reader: Reader,
    State(app): State<AppState>,
) -> Result<Json<Problems>, AppError> {
    Ok(Json(repo::problems(&app.pool).await?))
}

#[derive(Debug, Deserialize)]
pub struct ReportsQuery {
    pub source: Option<String>,
    pub collector: Option<String>,
    pub limit: Option<u32>,
}

/// GET /api/reports — report history (runs), newest first, optional filters.
pub async fn reports(
    _user: AuthUser,
    State(app): State<AppState>,
    Query(q): Query<ReportsQuery>,
) -> Result<Json<Vec<ReportSummary>>, AppError> {
    // Clamp the page size: a sane default, a hard ceiling to bound the response.
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    Ok(Json(
        repo::list_reports(
            &app.pool,
            q.source.as_deref(),
            q.collector.as_deref(),
            limit,
        )
        .await?,
    ))
}

/// GET /api/reports/:id — one report with all its checks.
pub async fn report(
    _user: AuthUser,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ReportDetail>, AppError> {
    repo::report_detail(&app.pool, &id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub source: String,
    pub collector: String,
    pub section: String,
    pub label: String,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

/// GET /api/history — time series for one check. Defaults to the last 30 days.
///
/// `Reader`, not `AuthUser`, since 2026-09-13. A board row is the LAST thing a
/// collector said, so for a daily collector it cannot answer "how often does this
/// actually fail?" — the question a flaky nightly turns on. The per-night verdicts
/// already existed in the `report` rows, behind a login an unattended reader
/// cannot perform, so the only alternative was one data point per night
/// (memview#1243).
///
/// ⚠ **Narrower than it looks, which is what made it acceptable.** It answers for
/// ONE fully specified key — source, collector, section AND label are all
/// required, none optional — so it enumerates nothing and cannot be swept for
/// what exists. `/api/overview` and `/api/reports` stay session-only precisely
/// because they DO enumerate.
pub async fn history(
    _reader: Reader,
    State(app): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<History>, AppError> {
    let to = q.to.unwrap_or_else(Utc::now);
    let from = q.from.unwrap_or_else(|| to - Duration::days(30));
    if from > to {
        return Err(AppError::BadRequest("from must be <= to".into()));
    }
    // Named struct fields, not four positional strings — a transposed
    // collector/section can't compile.
    let key = CheckKey {
        source: q.source,
        collector: q.collector,
        section: q.section,
        label: q.label,
    };
    Ok(Json(repo::history(&app.pool, &key, from, to).await?))
}
