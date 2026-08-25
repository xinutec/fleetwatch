//! Retirement CRUD — declaring a producer finished without discarding what it
//! measured. Like mutes, all three endpoints require a Nextcloud-login session
//! (`AuthUser`): retiring a producer removes it from the fleet's attention
//! permanently, so it is an accountable human decision and `created_by` is
//! stamped from the session. A read token (the unattended poller) can never
//! create one.
//!
//! The route this replaces was root ssh → `kubectl exec` → a hand-written
//! `DELETE`, which could not separate "stop reporting this" from "forget this
//! ever ran" and destroyed 63,225 rows the one time it was used.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::error::AppError;
use crate::report::repo;
use crate::report::types::{NewRetirement, Retirement};
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/retirements — every retired producer, newest first.
pub async fn list(
    _user: AuthUser,
    State(app): State<AppState>,
) -> Result<Json<Vec<Retirement>>, AppError> {
    Ok(Json(repo::list_retirements(&app.pool).await?))
}

/// POST /api/retirements — retire a producer, attributed to the session user.
/// 201 on success; 400 if identity/reason is empty. Idempotent: retiring an
/// already-retired producer restates the reason and keeps the original
/// `retired_at` (see `repo::create_retirement`).
pub async fn create(
    AuthUser(user): AuthUser,
    State(app): State<AppState>,
    Json(new): Json<NewRetirement>,
) -> Result<(StatusCode, Json<Retirement>), AppError> {
    let r = repo::create_retirement(&app.pool, &new, &user.user_id).await?;
    tracing::info!(
        by = %user.user_id, source = %r.source, collector = %r.collector,
        "retired since {}", r.retired_at
    );
    Ok((StatusCode::CREATED, Json(r)))
}

/// DELETE /api/retirements/:source/:collector — un-retire. 204 if removed, 404
/// if it was not retired. The producer counts as stale again from the next read.
pub async fn delete(
    _user: AuthUser,
    State(app): State<AppState>,
    Path((source, collector)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    if repo::delete_retirement(&app.pool, &source, &collector).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}
