//! Readiness against a real database: the positive half of the probe split.
//!
//! `tests/routes_http.rs` proves `/readyz` reports 503 when the database is
//! unreachable. That alone would pass on a handler that always answered 503,
//! which is the same defect as always answering 200 pointed the other way. This
//! asserts the probe distinguishes the two states rather than picking one.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fleetwatch::config::Config;
use fleetwatch::routes;
use fleetwatch::state::AppState;
use tower::ServiceExt;

fn cfg(url: &str) -> Config {
    Config {
        database_url: url.into(),
        bind_addr: "0.0.0.0:0".into(),
        static_dir: None,
        tokens: vec![],
        read_tokens: vec![],
        raw_retention_days: 30,
        check_retention_days: 400,
        session_secret: "test-session-secret".into(),
        nc_base_url: "https://nc.example".into(),
        nc_client_id: "test-client".into(),
        nc_client_secret: "test-secret".into(),
        nc_redirect_uri: "https://fleetwatch.example/auth/callback".into(),
        dev_login_user: None,
    }
}

#[tokio::test]
async fn readyz_is_200_against_a_live_database() {
    let Some((pool, _guard)) = common::setup("readyz-test").await else {
        return;
    };
    let url = std::env::var("FLEETWATCH_TEST_DATABASE_URL").unwrap();
    let app = routes::router(AppState::new(pool, cfg(&url), reqwest::Client::new()));

    let res = app
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
