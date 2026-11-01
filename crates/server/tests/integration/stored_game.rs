//! A stored game is its creation inputs plus its action log (ADR 0022): a
//! reload re-runs setup from those inputs and replays the log, and a game it
//! can't rebuild is reported as an unresumable game with its rows kept.
//!
//! Drift is simulated by editing one stored row with SQL between `apply` and
//! `load`, which is the observable a real engine change produces.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId, Responder};
use game_core::event::Event;
use game_core::state::game_state::GameState;
use game_core::state::investigator::InvestigatorId;
use protocol::logged_answer::ReplayFailure;
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
    session.apply(keep_opening_hand()).await.expect("accepted");

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("played"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.live().state(), session.live().state());
    assert_eq!(loaded.live().outcome(), session.live().outcome());
    assert_eq!(loaded.live().setup_events(), session.live().setup_events());
}

#[tokio::test]
async fn no_stored_column_holds_serialized_engine_state() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "inputs-only").await;
    session.apply(keep_opening_hand()).await.expect("accepted");

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
    session.apply(keep_opening_hand()).await.expect("accepted");
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
    session.apply(keep_opening_hand()).await.expect("accepted");
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
async fn a_game_whose_setup_no_longer_runs_is_unresumable() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "unregistered").await;
    session.apply(keep_opening_hand()).await.expect("accepted");
    // As if a PR had removed the game's scenario from the registry.
    sqlx::query("UPDATE games SET scenario_id = 'no-such-scenario' WHERE game_id = ?")
        .bind("unregistered")
        .execute(&pool)
        .await
        .unwrap();

    let reason = expect_unresumable(&pool, "unregistered").await;

    assert_eq!(
        reason,
        UnresumableReason::Setup("unknown scenario: no-such-scenario".to_owned()),
    );
    assert_eq!(common::stored_rows(&pool, "unregistered").await, (1, 1));
}

#[tokio::test]
async fn a_game_stored_in_another_format_version_is_a_version_error() {
    // 1 is the format before a logged answer named what it answered.
    for version in [0_i64, 1, 99] {
        let pool = common::memory_pool().await;
        let mut session = common::create_game(&pool, "versioned").await;
        session.apply(keep_opening_hand()).await.expect("accepted");
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
async fn a_logged_pick_finds_its_option_by_label_not_by_id() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "reordered").await;
    session.apply(keep_opening_hand()).await.expect("accepted");
    let gain = turn_option(&session, "Gain resource");
    let end_turn = turn_option(&session, "End turn");
    session.apply(pick(gain)).await.expect("accepted");
    // As if a PR had reordered the turn menu: the logged pick now carries the
    // id that is End turn's, but still names Gain resource.
    common::set_logged(
        &pool,
        "reordered",
        1,
        "$.response.PickSingle.option.id",
        end_turn.0,
    )
    .await;

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("reordered"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.live().state(), session.live().state());
    assert_eq!(loaded.live().outcome(), session.live().outcome());
}

#[tokio::test]
async fn a_logged_pick_the_prompt_no_longer_offers_is_unresumable() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "renamed").await;
    session.apply(keep_opening_hand()).await.expect("accepted");
    session
        .apply(pick(turn_option(&session, "Gain resource")))
        .await
        .expect("accepted");
    // As if a PR had renamed the option.
    common::set_logged(
        &pool,
        "renamed",
        1,
        "$.response.PickSingle.option.label",
        "Take a resource",
    )
    .await;

    let reason = expect_unresumable(&pool, "renamed").await;

    assert!(
        matches!(
            &reason,
            UnresumableReason::Replay {
                seq: 1,
                reason: ReplayFailure::OptionNotFound { option, matches: 0 },
            } if option.label == "Take a resource"
        ),
        "the pick finds no option at seq 1, got {reason:?}",
    );
    assert_eq!(common::stored_rows(&pool, "renamed").await, (1, 2));
}

#[tokio::test]
async fn an_answer_to_a_changed_prompt_is_unresumable() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "reprompted").await;
    session.apply(keep_opening_hand()).await.expect("accepted");
    session
        .apply(pick(turn_option(&session, "Gain resource")))
        .await
        .expect("accepted");
    // As if the turn menu had been asked of someone else when it was logged.
    common::set_logged(
        &pool,
        "reprompted",
        1,
        "$.prompt.responder",
        Responder::Investigator(InvestigatorId(2)),
    )
    .await;

    let reason = expect_unresumable(&pool, "reprompted").await;

    assert!(
        matches!(
            &reason,
            UnresumableReason::Replay {
                seq: 1,
                reason: ReplayFailure::PromptChanged { recorded, now: Some(now) },
            } if recorded.responder == Responder::Investigator(InvestigatorId(2))
                && now.responder == Responder::Investigator(InvestigatorId(1))
        ),
        "the prompt differs at seq 1, got {reason:?}",
    );
    assert_eq!(common::stored_rows(&pool, "reprompted").await, (1, 2));
}

#[tokio::test]
async fn an_action_that_now_draws_differently_is_unresumable() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "redrawn").await;
    session.apply(keep_opening_hand()).await.expect("accepted");
    session
        .apply(pick(turn_option(&session, "Gain resource")))
        .await
        .expect("accepted");
    let draws = session.live().state().rng.draws;
    // As if the action had drawn once more when it was logged.
    sqlx::query("UPDATE actions SET draws = draws + 1 WHERE game_id = ? AND seq = 1")
        .bind("redrawn")
        .execute(&pool)
        .await
        .unwrap();

    let reason = expect_unresumable(&pool, "redrawn").await;

    assert_eq!(
        reason,
        UnresumableReason::Replay {
            seq: 1,
            reason: ReplayFailure::Draws {
                recorded: draws + 1,
                now: draws,
            },
        },
    );
    assert_eq!(common::stored_rows(&pool, "redrawn").await, (1, 2));
}

#[tokio::test]
async fn an_unknown_game_is_absent_not_unresumable() {
    let pool = common::memory_pool().await;

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("nope"))
        .await
        .expect("an unknown game is not an error");

    assert!(loaded.is_none());
}

fn pick(option: OptionId) -> PlayerAction {
    PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(option),
    }
}

/// The id of the option labelled `label` on the prompt `session` awaits.
fn turn_option(session: &GameSession, label: &str) -> OptionId {
    let EngineOutcome::AwaitingInput { request, .. } = &session.live().outcome() else {
        panic!("expected a prompt, got {:?}", session.live().outcome());
    };
    request
        .options
        .iter()
        .find(|option| option.label == label)
        .unwrap_or_else(|| panic!("no `{label}` option in {:?}", request.options))
        .id
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
