//! Router-level tests that need no database: they drive the app via
//! `oneshot` and exercise paths that return before touching the pool (healthz,
//! auth rejection, schema rejection). The pool is created lazily so no DB
//! connection is made — these run in a plain `cargo test`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fleetwatch::config::Config;
use fleetwatch::routes;
use fleetwatch::state::AppState;
use tower::ServiceExt;

fn app() -> axum::Router {
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        // connect_lazy never dials until a query runs; the tests below never
        // reach a query, so no MariaDB is required.
        .connect_lazy("mysql://unused:unused@127.0.0.1:1/unused")
        .expect("lazy pool");
    let cfg = Config {
        database_url: "unused".into(),
        bind_addr: "0.0.0.0:0".into(),
        static_dir: None,
        tokens: vec![("mac-mini".into(), "secret-token".into())],
        read_tokens: vec!["read-token-0123456789abcdef".into()],
        raw_retention_days: 30,
        check_retention_days: 400,
        session_secret: "test-session-secret".into(),
        nc_base_url: "https://nc.example".into(),
        nc_client_id: "test-client".into(),
        nc_client_secret: "test-secret".into(),
        nc_redirect_uri: "https://fleetwatch.example/auth/callback".into(),
        dev_login_user: None,
    };
    routes::router(AppState::new(pool, cfg, reqwest::Client::new()))
}

#[tokio::test]
async fn healthz_ok() {
    let res = app()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn ingest_without_token_is_401() {
    let res = app()
        .oneshot(
            Request::post("/api/reports")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"schema":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_with_wrong_token_is_401() {
    let res = app()
        .oneshot(
            Request::post("/api/reports")
                .header("authorization", "Bearer wrong")
                .body(Body::from(r#"{"schema":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn read_endpoint_without_session_is_401() {
    // The human read side is gated by the NC-login session (AuthUser). The
    // extractor rejects before touching the pool, so no DB is needed here.
    let res = app()
        .oneshot(Request::get("/api/overview").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn problems_without_any_credential_is_401() {
    // /api/problems is the one endpoint a read token can reach. With no credential at
    // all it must still 401 — the token widens who may read, never whether auth applies.
    let res = app()
        .oneshot(Request::get("/api/problems").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn problems_with_a_wrong_read_token_is_401() {
    let res = app()
        .oneshot(
            Request::get("/api/problems")
                .header("authorization", "Bearer not-the-read-token-xxxxxx")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn problems_with_a_valid_read_token_passes_auth() {
    // Auth must succeed and let the handler run (it then fails on the dud pool — a 5xx,
    // NOT a 401). That distinction is the whole assertion: the poller got through.
    let res = app()
        .oneshot(
            Request::get("/api/problems")
                .header("authorization", "Bearer read-token-0123456789abcdef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_token_cannot_reach_any_other_endpoint() {
    // Least privilege, and the test that keeps it that way: the phone's token opens
    // /api/problems and nothing else. If someone later swaps another handler to
    // `Reader`, this fails.
    for path in ["/api/overview", "/api/reports", "/api/history"] {
        let res = app()
            .oneshot(
                Request::get(path)
                    .header("authorization", "Bearer read-token-0123456789abcdef")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{path} must stay session-only"
        );
    }
}

#[tokio::test]
async fn a_producer_ingest_token_is_not_a_read_token() {
    // The two token sets are separate namespaces. A producer's write token must not
    // become a read credential just because both arrive as `Authorization: Bearer`.
    let res = app()
        .oneshot(
            Request::get("/api/problems")
                .header("authorization", "Bearer secret-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authed_but_unparseable_body_is_422() {
    let res = app()
        .oneshot(
            Request::post("/api/reports")
                .header("authorization", "Bearer secret-token")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// `/readyz` reports 503 when the database cannot be reached.
///
/// The lazy pool above points at 127.0.0.1:1, where nothing listens, so this is
/// the real failure the probe exists to catch rather than a mocked one. It is
/// also the test that would have failed on the old handler: `/healthz` returned
/// the literal "ok" and could not distinguish a serving app from a wedged one,
/// so k8s readiness passed while every read 500'd.
#[tokio::test]
async fn readyz_is_503_when_the_database_is_unreachable() {
    let res = app()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Liveness stays shallow ON PURPOSE, and this test is what says so.
///
/// `/healthz` must keep answering 200 with the same dead pool that makes
/// `/readyz` fail above. Liveness failing on a database outage would restart the
/// app in a loop while the thing it depends on is down — the restart fixes
/// nothing and costs the recovery. Readiness is where "cannot serve" belongs,
/// because it stops traffic without killing the process.
#[tokio::test]
async fn healthz_stays_up_when_the_database_is_unreachable() {
    let res = app()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
