//! Game lifecycle HTTP: `POST /games` creates a game as a lobby and returns
//! its id; the lobby is stored, with its creator as its first member, so a
//! later WS connect to the id finds it.

use axum::body;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use protocol::GameId;
use serde_json::Value;
use tower::ServiceExt as _;

use crate::common::{self, TEST_SCENARIO_ID};

/// Post `body` to `POST /games` as the signed-in [`common::TEST_ACCOUNT`],
/// over a fresh database; return the response's status and JSON body, and
/// the database.
async fn post_games(body: &str) -> (StatusCode, Value, sqlx::SqlitePool) {
    let pool = common::memory_pool().await;
    let (app, cookie) = common::signed_in_app(pool.clone(), common::REGISTRIES).await;
    let request = Request::builder()
        .method("POST")
        .uri("/games")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, pool)
}

#[tokio::test]
async fn post_games_creates_a_lobby_and_returns_its_id() {
    let (status, json, pool) =
        post_games(&format!(r#"{{"scenario_id":"{TEST_SCENARIO_ID}"}}"#)).await;
    assert_eq!(status, StatusCode::CREATED);
    let game_id = json["game_id"].as_str().expect("game_id is a string");

    // The lobby is stored, and its creator is its first member.
    let creator = common::test_account(&pool).await;
    let members = server::lobby::members_of(&pool, &GameId::new(game_id))
        .await
        .expect("read the members");
    assert_eq!(members, [creator]);
}

#[tokio::test]
async fn post_games_unknown_scenario_is_bad_request() {
    let (status, _, _) = post_games(r#"{"scenario_id":"no-such-scenario"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
