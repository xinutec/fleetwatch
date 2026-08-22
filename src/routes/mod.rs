//! HTTP routing table. One token-authed write (POST /api/reports); the read
//! endpoints require a Nextcloud-login session; the built Angular bundle is
//! served single-origin with an index.html SPA fallback.

pub mod auth;
pub mod health;
pub mod ingest;
pub mod mutes;
pub mod telemetry;
pub mod views;

use axum::Router;
use axum::routing::{delete, get, post};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/reports", post(ingest::create).get(views::reports))
        .route("/reports/{id}", get(views::report))
        .route("/overview", get(views::overview))
        .route("/problems", get(views::problems))
        .route("/history", get(views::history))
        .route("/mutes", get(mutes::list).post(mutes::create))
        .route("/mutes/{id}", delete(mutes::delete))
        // What the person did, folded into the same log as what the API saw.
        .route("/telemetry", post(telemetry::record))
        // One INFO line per API request. Scoped to /api so static-asset serving
        // and the k8s liveness/readiness probes don't spam the log.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    let mut app = Router::new()
        // Two questions, not one: shallow liveness, database-backed
        // readiness. See routes::health.
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // DEV ONLY: mount /dev-login only when DEV_LOGIN_USER is set.
    if state.cfg.dev_login_user.is_some() {
        app = app.route("/dev-login", get(auth::dev_login));
    }

    // Serve the built Angular bundle (single origin), falling back to index.html
    // so client-side routes resolve. API-only when STATIC_DIR is unset (dev,
    // where `ng serve` proxies /api).
    if let Some(dir) = state.cfg.static_dir.clone() {
        let serve = ServeDir::new(&dir).fallback(ServeFile::new(format!("{dir}/index.html")));
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
