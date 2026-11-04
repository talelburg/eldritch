//! Game lifecycle HTTP: `POST /games` creates a game and returns its id;
//! the created game is persisted and loadable (proving the lazy-rehydrate
//! path a later WS connect relies on).

use axum::body;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use game_core::state::InvestigatorId;
use game_core::test_support::TEST_INV;
use protocol::{Assignment, GameId};
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

/// The assignment is stored beside the game, so a fresh load over the same
/// database, as after a restart, gets back the one `POST /games` created the
/// game with: the signed-in creator holds every investigator.
#[tokio::test]
async fn a_game_created_through_post_reloads_with_the_assignment_it_was_created_with() {
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

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new(game_id))
        .await
        .unwrap()
        .expect("the created game loads");

    assert_eq!(
        loaded.live().assignment(),
        &Assignment::from_iter([(InvestigatorId(1), creator)])
    );
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
