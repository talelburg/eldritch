//! Signing in: the session cookie, `/auth/me`, sign-out, the dev picker and
//! its guards, and the sign-in `POST /games` and a game's WebSocket require.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use game_core::test_support::TEST_INV;
use protocol::{SignInMethod, SignInRequired, SignedIn};
use server::{auth, AppState};
use sqlx::SqlitePool;
use tokio::time;
use tower::ServiceExt as _;

use crate::common::{self, TEST_ACCOUNT, TEST_SCENARIO_ID};

/// Serve `pool` with the dev sign-in `on` or off.
async fn serve(pool: SqlitePool, dev_auth: bool) -> SocketAddr {
    common::serve(server::app(
        AppState::new(pool, common::REGISTRIES).with_dev_auth(dev_auth),
    ))
    .await
}

/// `GET /auth/me` at `addr`, with `cookie` if there is one.
async fn me(addr: SocketAddr, cookie: Option<&str>) -> reqwest::Response {
    let mut request = common::http().get(format!("http://{addr}/auth/me"));
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    request.send().await.expect("GET /auth/me")
}

/// The sign-in methods a `401` from `/auth/me` lists.
async fn methods(response: reqwest::Response) -> Vec<SignInMethod> {
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: SignInRequired =
        serde_json::from_slice(&response.bytes().await.expect("a body")).expect("a method list");
    body.methods
}

/// The status `app`, serving the fixture bundle, answers `method path` with
/// `host` as the `Host`. The bundle is there so a path that fell through to
/// the client bundle would answer `200`, which a missing bundle would hide.
async fn status(dev_auth: bool, method: &str, path: &str, host: &str) -> StatusCode {
    let app = server::app(
        AppState::new_with_dist(
            common::memory_pool().await,
            common::REGISTRIES,
            common::FIXTURE_DIST.into(),
        )
        .with_dev_auth(dev_auth),
    );
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("account_id=1"))
        .expect("build the request");
    app.oneshot(request).await.expect("a response").status()
}

#[tokio::test]
async fn without_the_flag_the_dev_routes_are_not_found() {
    for method in ["GET", "POST"] {
        assert_eq!(
            status(false, method, "/auth/dev", "localhost").await,
            StatusCode::NOT_FOUND,
            "{method} /auth/dev"
        );
    }
}

#[tokio::test]
async fn with_the_flag_the_dev_routes_are_not_found_for_a_public_host() {
    for method in ["GET", "POST"] {
        for host in ["eldritch.gripe", "eldritch.gripe:443"] {
            assert_eq!(
                status(true, method, "/auth/dev", host).await,
                StatusCode::NOT_FOUND,
                "{method} /auth/dev for {host}"
            );
        }
    }
}

#[tokio::test]
async fn with_the_flag_the_dev_picker_answers_every_loopback_host() {
    for host in [
        "localhost",
        "localhost:8000",
        "127.0.0.1:3000",
        "[::1]:8000",
    ] {
        assert_eq!(
            status(true, "GET", "/auth/dev", host).await,
            StatusCode::OK,
            "{host}"
        );
    }
}

#[tokio::test]
async fn an_unrouted_auth_path_is_not_found() {
    assert_eq!(
        status(true, "GET", "/auth/nowhere", "localhost").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn signed_out_me_lists_the_dev_picker_when_it_is_on() {
    let addr = serve(common::memory_pool().await, true).await;

    assert_eq!(
        methods(me(addr, None).await).await,
        vec![SignInMethod {
            label: "Dev sign-in".to_string(),
            url: "/auth/dev".to_string(),
        }]
    );
}

#[tokio::test]
async fn signed_out_me_lists_no_method_without_the_dev_picker() {
    let addr = serve(common::memory_pool().await, false).await;

    assert_eq!(methods(me(addr, None).await).await, vec![]);
}

#[tokio::test]
async fn picking_an_account_on_the_dev_picker_signs_in_as_it() {
    let pool = common::memory_pool().await;
    common::add_test_account(&pool).await;
    let addr = serve(pool, true).await;

    let cookie = common::sign_in_over_http(addr, TEST_ACCOUNT).await;

    let response = me(addr, Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let signed_in: SignedIn =
        serde_json::from_slice(&response.bytes().await.expect("a body")).expect("an account");
    assert_eq!(signed_in.display_name, TEST_ACCOUNT);
}

/// Signed out, creating a game is refused whatever the body: one that would
/// create a game, and one the server couldn't read at all.
#[tokio::test]
async fn creating_a_game_signed_out_is_unauthorized() {
    let addr = serve(common::memory_pool().await, true).await;

    for body in [
        format!(
            r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[],"random_basic_weaknesses":0}}],"lead":0}}"#
        ),
        "{}".to_string(),
    ] {
        let response = common::http()
            .post(format!("http://{addr}/games"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.clone())
            .send()
            .await
            .expect("POST /games");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{body}");
    }
}

#[tokio::test]
async fn a_socket_without_a_session_is_closed_as_unauthenticated() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-signed-out").await;
    let addr = serve(pool, true).await;

    let mut ws = common::connect_with(addr, "g-signed-out", None).await;

    common::assert_closed_unauthenticated(&mut ws).await;
}

#[tokio::test]
async fn a_socket_with_an_unknown_session_is_closed_as_unauthenticated() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-unknown").await;
    let addr = serve(pool, true).await;

    let cookie = format!("{}=not-a-session", auth::SESSION_COOKIE);
    let mut ws = common::connect_with(addr, "g-unknown", Some(&cookie)).await;

    common::assert_closed_unauthenticated(&mut ws).await;
}

#[tokio::test]
async fn an_expired_session_signs_in_as_nobody() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-expired").await;
    let addr = common::spawn_server(pool.clone()).await;
    let cookie = common::sign_in_over_http(addr, TEST_ACCOUNT).await;

    expire_every_session(&pool).await;

    assert_eq!(
        me(addr, Some(&cookie)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let mut ws = common::connect_with(addr, "g-expired", Some(&cookie)).await;
    common::assert_closed_unauthenticated(&mut ws).await;
}

/// Each request a session authenticates moves its expiry out again, so a
/// session in use outlives the expiry it had when the request came in.
#[tokio::test]
async fn a_request_slides_the_session_s_expiry() {
    let pool = common::memory_pool().await;
    let addr = common::spawn_server(pool.clone()).await;
    let cookie = common::sign_in_over_http(addr, TEST_ACCOUNT).await;
    expire_every_session_in(&pool, Duration::from_millis(500)).await;

    assert_eq!(me(addr, Some(&cookie)).await.status(), StatusCode::OK);
    time::sleep(Duration::from_secs(1)).await;

    assert_eq!(me(addr, Some(&cookie)).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn signing_out_ends_the_session_and_clears_the_cookie() {
    let addr = common::spawn_server(common::memory_pool().await).await;
    let cookie = common::sign_in_over_http(addr, TEST_ACCOUNT).await;

    let response = sign_out(addr, Some(&cookie)).await;

    assert!(response.status().is_success(), "{}", response.status());
    let cleared = common::session_cookie(response.headers());
    assert_eq!(cleared, format!("{}=", auth::SESSION_COOKIE));
    assert_eq!(
        me(addr, Some(&cookie)).await.status(),
        StatusCode::UNAUTHORIZED,
        "the old cookie signs in as nobody"
    );
}

#[tokio::test]
async fn signing_out_without_a_session_succeeds() {
    let addr = serve(common::memory_pool().await, false).await;

    assert!(sign_out(addr, None).await.status().is_success());
}

/// `POST /auth/sign-out` at `addr`, with `cookie` if there is one.
async fn sign_out(addr: SocketAddr, cookie: Option<&str>) -> reqwest::Response {
    let mut request = common::http().post(format!("http://{addr}/auth/sign-out"));
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    request.send().await.expect("POST /auth/sign-out")
}

/// End every session in `pool` now.
async fn expire_every_session(pool: &SqlitePool) {
    expire_every_session_in(pool, Duration::ZERO).await;
}

/// Set every session in `pool` to expire `from_now`: the stored-row edit that
/// stands in for the time passing.
async fn expire_every_session_in(pool: &SqlitePool, from_now: Duration) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch");
    let at = i64::try_from((now + from_now).as_millis()).expect("millis fit in i64");
    sqlx::query("UPDATE sessions SET expires_at = ?")
        .bind(at)
        .execute(pool)
        .await
        .expect("expire the sessions");
}
