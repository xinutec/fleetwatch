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
use axum::http::{HeaderValue, Response, header};
use axum::routing::{delete, get, post};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// How long a static response may be reused without asking again.
///
/// ⚠ **`index.html` MUST REVALIDATE.** With no `Cache-Control` at all — which
/// is what this served from 2026-07-04 until it was measured on 2026-09-07 — a
/// client falls back to HEURISTIC freshness, roughly a tenth of the document's
/// age, and may keep it for days without ever asking. The document names the
/// content-hashed bundle, so the new `main-*.js` is never fetched either and a
/// deploy is invisible: an Android `WebView` loaded messages' API and a whole
/// thread while running several builds behind, with a missing button as the
/// only symptom.
///
/// `no-cache` means "ask first", not "never keep" — the `ETag` still turns the
/// usual case into a 304 with no body.
///
/// Everything else Angular emits carries a content hash in its NAME, so a new
/// build is a new URL and the old one can never be wrong. Those are the one
/// kind of response `immutable` is honestly available for.
///
/// Generic over the body: `ServeDir`'s response body type depends on what it
/// falls back to, and this predicate only ever reads a header.
fn cache_control_for<B>(res: &Response<B>) -> Option<HeaderValue> {
    // ⚠ **A 404 is not an asset.** `SetResponseHeaderLayer::overriding` stamps
    // whatever the service returned, and a missing file answered with a year of
    // `immutable` is a client that will not ask for that name again this year.
    // Only a response that carried something may say how long it keeps.
    //
    // ⚠ NOT `!is_success()`. That excludes **304 Not Modified**, which must
    // carry the headers a 200 would so the client can refresh what it already
    // holds. Stripping it made every revalidated image a full re-fetch, and the
    // arriving bytes grew the thread AFTER it had scrolled to the bottom —
    // `thread-scroll.spec.ts` caught it at 271px off, a symptom with no visible
    // connection to a cache header.
    if res.status().is_client_error() || res.status().is_server_error() {
        return None;
    }
    let is_html = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    Some(if is_html {
        HeaderValue::from_static("no-cache")
    } else {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    })
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
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                cache_control_for,
            ))
            .service(serve);
        app = app.fallback_service(serve);
    }

    app.with_state(state)
}
