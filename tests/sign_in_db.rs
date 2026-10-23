//! A whole sign-in through the real router, against a stand-in Nextcloud that
//! grants any code: the cookies it leaves behind. The session is a row, so this
//! skips without FLEETWATCH_TEST_DATABASE_URL.

mod common;

use axum::Json;
use axum::body::Body;
use axum::http::{Request, header};
use axum::response::Response;
use axum::routing::{get, post};
use fleetwatch::config::Config;
use fleetwatch::state::AppState;
use fleetwatch::{pending_login, routes, session};
use tower::ServiceExt;

/// A Nextcloud that trades any code for a token naming `user`.
async fn nextcloud() -> String {
    let nc = axum::Router::new()
        .route(
            "/index.php/apps/oauth2/api/v1/token",
            post(|| async { Json(serde_json::json!({ "access_token": "token" })) }),
        )
        .route(
            "/ocs/v2.php/cloud/user",
            get(|| async {
                Json(serde_json::json!({ "ocs": { "data": { "id": "user", "displayname": "User" } } }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, nc).await.unwrap() });
    format!("http://{addr}")
}

async fn send(app: &axum::Router, req: Request<Body>) -> Response {
    app.clone().oneshot(req).await.unwrap()
}

/// The `Set-Cookie` line for `name`, which a test expects to find.
fn cookie_line(res: &Response, name: &str) -> String {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .find(|l| l.starts_with(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no Set-Cookie for {name}: {:?}", res.headers()))
}

/// `name=value`, as a browser sends it back.
fn pair(res: &Response, name: &str) -> String {
    cookie_line(res, name).split(';').next().unwrap().to_owned()
}

fn assert_removed_at_root(res: &Response, name: &str) {
    let dropped = cookie_line(res, name);
    assert!(dropped.contains("Max-Age=0"), "{dropped}");
    assert!(
        dropped.contains("Path=/"),
        "removed where it was set: {dropped}"
    );
}

/// A cookie set at `Path=/` is removed only by a removal at `Path=/`. Without
/// one, the browser scopes the removal to the request's directory — `/auth`
/// for the callback — and the cookie it meant to drop stays.
#[tokio::test]
async fn signing_in_and_out_removes_each_cookie_where_it_was_set() {
    let Some((pool, _guard)) = common::setup("sign-in-test").await else {
        return;
    };
    let cfg = Config {
        database_url: String::new(),
        bind_addr: "0.0.0.0:0".into(),
        static_dir: None,
        tokens: vec![],
        read_tokens: vec![],
        raw_retention_days: 30,
        check_retention_days: 400,
        session_secret: "test-session-secret".into(),
        nc_base_url: nextcloud().await,
        nc_client_id: "test-client".into(),
        nc_client_secret: "test-secret".into(),
        nc_redirect_uri: "https://fleetwatch.example/auth/callback".into(),
        dev_login_user: None,
    };
    let app = routes::router(AppState::new(pool, cfg, reqwest::Client::new()));

    let login = send(&app, Request::get("/login").body(Body::empty()).unwrap()).await;
    let pending = pair(&login, pending_login::COOKIE_NAME);
    let to_nc = login.headers()[header::LOCATION].to_str().unwrap();
    let state = url::Url::parse(to_nc)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .expect("the authorize URL carries a state");

    let back = send(
        &app,
        Request::get(format!("/auth/callback?code=granted&state={state}"))
            .header(header::COOKIE, &pending)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(back.status().is_redirection(), "{}", back.status());
    assert_removed_at_root(&back, pending_login::COOKIE_NAME);
    let signed_in = pair(&back, session::COOKIE_NAME);

    let out = send(
        &app,
        Request::post("/logout")
            .header(header::COOKIE, &signed_in)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(out.status().is_redirection(), "{}", out.status());
    assert_removed_at_root(&out, session::COOKIE_NAME);
}
