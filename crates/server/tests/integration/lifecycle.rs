//! Game lifecycle HTTP: `POST /games` creates a game and returns its id;
//! the created game is persisted and loadable (proving the lazy-rehydrate
//! path a later WS connect relies on).

use axum::body;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use game_core::test_support::TEST_INV;
use protocol::GameId;
use serde_json::Value;
use server::session::GameSession;
use tower::ServiceExt as _;

use crate::common::{self, TEST_SCENARIO_ID};

#[tokio::test]
async fn post_games_creates_game_and_returns_id() {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool.clone(), common::REGISTRIES).await;

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(format!(
            r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[],"random_basic_weaknesses":0}}],"lead":0}}"#
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    let game_id = json["game_id"].as_str().expect("game_id is a string");

    // The created game is persisted and loadable.
    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new(game_id))
        .await
        .unwrap();
    assert!(
        loaded.is_some(),
        "POSTed game should be loadable from the log"
    );
}

#[tokio::test]
async fn a_created_game_records_its_creator_who_holds_every_investigator() {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool.clone(), common::REGISTRIES).await;
    let creator = common::test_account(&pool).await;

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(format!(
            r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[],"random_basic_weaknesses":0}}],"lead":0}}"#
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    let game_id = json["game_id"].as_str().expect("game_id is a string");

    // Loaded back from its stored rows, so the creator came from the store.
    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new(game_id))
        .await
        .unwrap()
        .expect("the created game loads");
    let live = loaded.live();
    assert!(!live.state().investigators.is_empty());
    for investigator in live.state().investigators.keys() {
        assert_eq!(
            live.assignment().holder(*investigator),
            Some(creator),
            "the signed-in creator holds {investigator:?}"
        );
    }
}

#[tokio::test]
async fn post_games_unknown_scenario_is_bad_request() {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool, common::REGISTRIES).await;

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(
            r#"{"scenario_id":"no-such-scenario","roster":[],"lead":0}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Post `body` to `POST /games` as the signed-in [`common::TEST_ACCOUNT`];
/// return the response's status.
async fn post_games_status(body: &str) -> StatusCode {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool, common::REGISTRIES).await;
    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn post_games_with_the_scenario_alone_creates_a_lobby() {
    let body = format!(r#"{{"scenario_id":"{TEST_SCENARIO_ID}"}}"#);
    assert_eq!(post_games_status(&body).await, StatusCode::CREATED);
}

#[tokio::test]
async fn a_lobby_of_an_unknown_scenario_is_bad_request() {
    let body = r#"{"scenario_id":"no-such-scenario"}"#;
    assert_eq!(post_games_status(body).await, StatusCode::BAD_REQUEST);
}

/// A roster and a lead come together or not at all.
#[tokio::test]
async fn a_roster_without_a_lead_or_a_lead_without_a_roster_is_bad_request() {
    let roster_only = format!(
        r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[],"random_basic_weaknesses":0}}]}}"#
    );
    let lead_only = format!(r#"{{"scenario_id":"{TEST_SCENARIO_ID}","lead":0}}"#);
    for body in [roster_only, lead_only] {
        assert_eq!(
            post_games_status(&body).await,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
}

/// A lead index past the roster names no investigator: the engine rejects the
/// seating, which creation reports as unprocessable.
#[tokio::test]
async fn post_games_with_a_lead_past_the_roster_is_unprocessable() {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool, common::REGISTRIES).await;

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(format!(
            r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[],"random_basic_weaknesses":0}}],"lead":1}}"#
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
