//! Mute CRUD — deliberate, expiring suppressions of a known problem. All three
//! endpoints require a Nextcloud-login session (`AuthUser`): a mute is an
//! accountable human decision, so `created_by` is stamped from the session and a
//! read token (the unattended poller) can never create one.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::error::AppError;
use crate::report::repo;
use crate::report::types::{Mute, NewMute};
use crate::session::AuthUser;
use crate::state::AppState;

/// GET /api/mutes — every live mute, newest first.
pub async fn list(
    _user: AuthUser,
    State(app): State<AppState>,
) -> Result<Json<Vec<Mute>>, AppError> {
    Ok(Json(repo::list_mutes(&app.pool).await?))
}

/// POST /api/mutes — create a mute attributed to the session user. 201 on
/// success; 400 if identity/reason is empty (see `repo::create_mute`).
pub async fn create(
    AuthUser(user): AuthUser,
    State(app): State<AppState>,
    Json(new): Json<NewMute>,
) -> Result<(StatusCode, Json<Mute>), AppError> {
    let mute = repo::create_mute(&app.pool, &new, &user.user_id).await?;
    tracing::info!(
        by = %user.user_id, source = %mute.source, collector = %mute.collector,
        label = %mute.label, "muted until {}", mute.expires_at
    );
    Ok((StatusCode::CREATED, Json(mute)))
}

/// DELETE /api/mutes/:id — unmute early. 204 if removed, 404 if it was already
/// gone (expired-and-swept, or never existed).
pub async fn delete(
    _user: AuthUser,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if repo::delete_mute(&app.pool, &id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}
