//! HTTP routing table. One token-authed write (POST /api/reports); the read
//! endpoints require a Nextcloud-login session; the built Angular bundle is
//! served single-origin with an index.html SPA fallback.

pub mod auth;
pub mod health;
pub mod ingest;
pub mod mutes;
pub mod retirements;
pub mod telemetry;
pub mod views;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{delete, get, post};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// A name carrying its content hash (`main-7GNM4EPX.js`, a font's `-ABCD1234`):
/// a new build is a new name, so it may be cached for ever.
fn is_content_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    let Some((stem, _ext)) = name.rsplit_once('.') else {
        return false;
    };
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() == 8
        && hash
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// How long a static response may be reused. A hashed file is `immutable`.
/// Everything else revalidates (`no-cache`: ask first, then a 304 via the ETag):
/// the page names the bundle, and an icon, the manifest and the service worker's
/// files keep their names across builds, so a year of `immutable` pins the old
/// one on every phone. Without `Cache-Control` a WebView caches heuristically and
/// runs a build several deploys old. An error is never `immutable`, or the name is
/// not asked for again; a 304 keeps what a 200 would say.
async fn cache_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let hashed = is_content_hashed(req.uri().path());
    let mut res = next.run(req).await;
    let failed = res.status().is_client_error() || res.status().is_server_error();
    let value = if hashed && !failed {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

/// Serve the app's page for a client-side ROUTE, and 404 anything that plainly
/// named a file.
///
/// ⚠ **A missing FILE must not be handed the page, and this mistake is
/// invisible**: the wrong answer is a `200`, so a browser that asked for a
/// woff2 and got HTML renders broken icons and reports nothing anywhere.
/// Measured here 2026-09-08 — `/media/nope.woff2` answered `200 text/html`
/// (#1478). `tasks` and memview's console both shipped it and were fixed this
/// way; this is the third copy.
///
/// The test is a dot in the last path segment. It is a heuristic, and the
/// alternative — enumerating the bundle's own asset names — would have to be
/// rebuilt whenever `ng build` changes a hash.
fn spa(index: &str, path: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;

    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    }
    match std::fs::read_to_string(index) {
        Ok(page) => axum::response::Html(page).into_response(),
        Err(error) => {
            // STATIC_DIR set with no index is a misconfigured deployment, and
            // saying so beats serving an empty page that looks like the app.
            tracing::error!("the app's index could not be read: {error}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no index").into_response()
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/reports", post(ingest::create).get(views::reports))
        .route("/reports/{id}", get(views::report))
        .route("/overview", get(views::overview))
        .route("/problems", get(views::problems))
        .route("/history", get(views::history))
        .route("/mutes", get(mutes::list).post(mutes::create))
        .route("/mutes/{id}", delete(mutes::delete))
        // Keyed on (source, collector) rather than an id: a retirement is about
        // a producer's identity, and there is exactly one per producer.
        .route(
            "/retirements",
            get(retirements::list).post(retirements::create),
        )
        .route(
            "/retirements/{source}/{collector}",
            delete(retirements::delete),
        )
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
        // dev-lint: allow-unauthenticated-route dev only: mounted when DEV_LOGIN_USER is set, which no deployed manifest sets
        app = app.route("/dev-login", get(auth::dev_login));
    }

    // Serve the built Angular bundle (single origin), falling back to index.html
    // so client-side routes resolve. API-only when STATIC_DIR is unset (dev,
    // where `ng serve` proxies /api).
    if let Some(dir) = state.cfg.static_dir.clone() {
        let index = format!("{dir}/index.html");
        let serve = ServeDir::new(&dir).fallback(get(move |uri: axum::http::Uri| {
            let index = index.clone();
            async move { spa(&index, uri.path()) }
        }));
        // ⚠ The layer wraps the STATIC SERVICE ALONE. `health`'s first attempt
        // hooked every route and stamped a year of `immutable` onto API JSON,
        // which is this bug pointing the other way.
        let serve = ServiceBuilder::new()
            .layer(axum::middleware::from_fn(cache_headers))
            .service(serve);
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
