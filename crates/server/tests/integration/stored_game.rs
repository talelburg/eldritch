//! A stored game is its creation inputs plus its action log (ADR 0022): a
//! reload re-runs setup from those inputs and replays the log, and a game it
//! can't rebuild is reported as an unresumable game with its rows kept.
//!
//! Drift is simulated by editing one stored row with SQL between `apply` and
//! `load`, which is the observable a real engine change produces.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::game_state::GameState;
use protocol::GameId;
use server::session::GameSession;
use sqlx::Row as _;

use crate::common;

fn keep_opening_hand() -> PlayerAction {
    PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple { selected: vec![] },
    }
}

#[tokio::test]
async fn a_played_game_resumes_at_the_same_state_and_outcome() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "played").await;
    session.apply(keep_opening_hand()).await.unwrap();

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("played"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.state, session.state);
    assert_eq!(loaded.outcome, session.outcome);
    assert_eq!(loaded.setup_events, session.setup_events);
}

#[tokio::test]
async fn no_stored_column_holds_serialized_engine_state() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "inputs-only").await;
    session.apply(keep_opening_hand()).await.unwrap();

    for (table, select) in [
        ("games", "SELECT * FROM games"),
        ("actions", "SELECT * FROM actions"),
    ] {
        let rows = sqlx::query(select)
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(!rows.is_empty(), "`{table}` holds the game");
        for row in rows {
            for column in 0..row.len() {
                // An INTEGER column doesn't decode as text, and holds no JSON.
                let Ok(text) = row.try_get::<String, _>(column) else {
                    continue;
                };
                assert!(
                    serde_json::from_str::<GameState>(&text).is_err()
                        && serde_json::from_str::<EngineOutcome>(&text).is_err()
                        && serde_json::from_str::<Vec<Event>>(&text).is_err(),
                    "`{table}` column {column} holds serialized engine state: {text}",
                );
            }
        }
    }
}
