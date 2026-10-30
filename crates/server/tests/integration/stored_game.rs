//! A stored game is its creation inputs plus its action log (ADR 0022): a
//! reload re-runs setup from those inputs and replays the log, and a game it
//! can't rebuild is reported as an unresumable game with its rows kept.
//!
//! Drift is simulated by editing one stored row with SQL between `apply` and
//! `load`, which is the observable a real engine change produces.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::game_state::GameState;
use game_core::state::investigator::InvestigatorId;
use protocol::GameId;
use server::fingerprint::{FingerprintMismatch, Zone};
use server::session::{GameSession, SessionError, UnresumableReason};
use sqlx::{Row as _, SqlitePool};

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
        let rows = sqlx::query(select).fetch_all(&pool).await.unwrap();
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

#[tokio::test]
async fn each_logged_action_records_when_it_was_applied() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "timed").await;
    let before = unix_millis();
    session.apply(keep_opening_hand()).await.unwrap();
    let after = unix_millis();

    let (at,): (i64,) = sqlx::query_as("SELECT at FROM actions WHERE game_id = ? AND seq = 0")
        .bind("timed")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        (before..=after).contains(&at),
        "`at` {at} is when the action was applied, between {before} and {after}",
    );
}

#[tokio::test]
async fn a_setup_that_now_deals_differently_is_unresumable_naming_the_zone() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "redealt").await;
    session.apply(keep_opening_hand()).await.unwrap();
    // The mock roster's deck is empty; record a card in it, as if setup had
    // dealt one when the game was created.
    sqlx::query(
        "UPDATE games SET setup_fingerprint = \
         json_set(setup_fingerprint, '$.investigators[0].deck', json('[\"01006\"]')) \
         WHERE game_id = ?",
    )
    .bind("redealt")
    .execute(&pool)
    .await
    .unwrap();

    let reason = expect_unresumable(&pool, "redealt").await;

    assert_eq!(
        reason,
        UnresumableReason::Fingerprint(FingerprintMismatch {
            zone: Zone::Deck(InvestigatorId(1)),
            position: 0,
        }),
    );
    assert_eq!(
        reason.to_string(),
        "setup now decides differently: investigator 1's deck differs at position 0",
    );
    assert_eq!(common::stored_rows(&pool, "redealt").await, (1, 1));
}

#[tokio::test]
async fn a_game_stored_in_another_format_version_is_a_version_error() {
    for version in [0_i64, 99] {
        let pool = common::memory_pool().await;
        let mut session = common::create_game(&pool, "versioned").await;
        session.apply(keep_opening_hand()).await.unwrap();
        sqlx::query("UPDATE games SET format_version = ? WHERE game_id = ?")
            .bind(version)
            .bind("versioned")
            .execute(&pool)
            .await
            .unwrap();

        let reason = expect_unresumable(&pool, "versioned").await;

        assert_eq!(reason, UnresumableReason::Version { found: version });
        assert_eq!(common::stored_rows(&pool, "versioned").await, (1, 1));
    }
}

#[tokio::test]
async fn an_unknown_game_is_absent_not_unresumable() {
    let pool = common::memory_pool().await;

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("nope"))
        .await
        .expect("an unknown game is not an error");

    assert!(loaded.is_none());
}

/// Load `game_id`, which must fail as unresumable under its own id, and
/// return the reason.
async fn expect_unresumable(pool: &SqlitePool, game_id: &str) -> UnresumableReason {
    match GameSession::load(pool.clone(), common::REGISTRIES, &GameId::new(game_id)).await {
        Err(SessionError::Unresumable {
            game_id: failed,
            reason,
        }) => {
            assert_eq!(failed.as_str(), game_id, "the failure names its game");
            reason
        }
        Err(other) => panic!("expected an unresumable game, got {other:?}"),
        Ok(_) => panic!("expected an unresumable game, but it loaded"),
    }
}

fn unix_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_millis();
    i64::try_from(millis).expect("millis fit an i64")
}
