//! Game lifecycle HTTP: `POST /games` creates a game and returns its id;
//! the created game is persisted and loadable (proving the lazy-rehydrate
//! path a later WS connect relies on).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{install_registry, memory_pool, TEST_SCENARIO_ID};
use game_core::test_support::TEST_INV;
use server::{GameId, GameSession};
use tower::ServiceExt;

#[tokio::test]
async fn post_games_creates_game_and_returns_id() {
    install_registry();
    let pool = memory_pool().await;
    let app = server::app(server::AppState::new(pool.clone()));

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"scenario_id":"{TEST_SCENARIO_ID}","roster":[{{"investigator":"{TEST_INV}","deck":[]}}]}}"#
        )))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let game_id = json["game_id"].as_str().expect("game_id is a string");

    // The created game is persisted and loadable.
    let loaded = GameSession::load(pool, &GameId::new(game_id))
        .await
        .unwrap();
    assert!(
        loaded.is_some(),
        "POSTed game should be loadable from the log"
    );
}

#[tokio::test]
async fn post_games_unknown_scenario_is_bad_request() {
    install_registry();
    let pool = memory_pool().await;
    let app = server::app(server::AppState::new(pool));

    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"scenario_id":"no-such-scenario","roster":[]}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
