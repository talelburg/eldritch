//! `GameSession` persistence: create-from-setup, apply-and-persist,
//! load-by-replay.
//!
//! Driven against an in-memory `SQLite` with a mock scenario registry (seating
//! runs at creation: a new game is round-1 mulligan-pending, so a
//! `ResolveInput(PickMultiple{selected:[]})` is accepted and a `ResolveInput`
//! selecting a non-existent card is rejected).

use game_core::action::{InputResponse, PlayerAction, RosterEntry};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::scenario::ScenarioId;
use game_core::state::{CardCode, InvestigatorId};
use protocol::GameId;
use server::logged_answer::ReplayFailure;
use server::session::{GameSession, SessionError, UnresumableReason};

use crate::common::{self, TEST_SCENARIO_ID};

/// Regression test for the load bug: a game with zero logged actions, whose
/// setup ends `AwaitingInput` at the mulligan, must load as `AwaitingInput`,
/// not `Done`.
#[tokio::test]
async fn load_restores_awaiting_input_seed_with_empty_log() {
    let pool = common::memory_pool().await;
    let session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "fresh",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("create");
    // create seats at creation → mulligan-pending, no actions logged yet.
    assert!(matches!(
        session.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(session.state.investigators.contains_key(&InvestigatorId(1)));

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("fresh"))
        .await
        .unwrap()
        .expect("exists");
    assert_eq!(
        loaded.state, session.state,
        "load reproduces the state setup left"
    );
    assert!(
        matches!(loaded.outcome, EngineOutcome::AwaitingInput { .. }),
        "load must restore setup's AwaitingInput outcome from an empty log, got {:?}",
        loaded.outcome
    );
}

#[tokio::test]
async fn create_persists_seed_and_exposes_setup_state() {
    let pool = common::memory_pool().await;

    let session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "game-1",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("create session");

    // create seats the roster: the investigator is present and round is 1.
    assert!(session.state.investigators.contains_key(&InvestigatorId(1)));
    assert_eq!(session.state.round, 1);
    assert_eq!(session.game_id.as_str(), "game-1");

    // A games row was persisted.
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM games WHERE game_id = ?")
        .bind("game-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn create_rejects_unknown_scenario() {
    let pool = common::memory_pool().await;

    let result = GameSession::create(
        pool,
        common::REGISTRIES,
        "game-x",
        ScenarioId::new("no-such-scenario"),
        vec![],
        0,
    )
    .await;

    assert!(matches!(result, Err(SessionError::UnknownScenario(_))));
}

#[tokio::test]
async fn create_rejects_bad_roster() {
    let pool = common::memory_pool().await;

    // Use an obviously-unknown investigator code; the synthetic registry
    // resolves nothing for it, so seating rejects.
    let bad_roster = vec![RosterEntry {
        investigator: CardCode::new("99999"),
        deck: vec![],
        random_basic_weaknesses: 0,
    }];

    let result = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "bad",
        ScenarioId::new(TEST_SCENARIO_ID),
        bad_roster,
        0,
    )
    .await;

    assert!(
        matches!(result, Err(SessionError::Seating(_))),
        "unknown investigator code must produce SessionError::Seating"
    );

    // The rejection must persist nothing: no games row for "bad".
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM games WHERE game_id = ?")
        .bind("bad")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "seating rejection must not persist a games row");
}

#[tokio::test]
async fn apply_persists_accepted_action_and_advances_state() {
    let pool = common::memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g2",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .unwrap();

    // create lands at round-1 mulligan-pending; resolve the mulligan (keep
    // the whole hand — empty redraw).
    let (_events, outcome) = session
        .apply(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })
        .await
        .unwrap();

    assert!(!matches!(outcome, EngineOutcome::Rejected { .. }));
    // After the mulligan resolves the mulligan queue must be empty (solo
    // game: only one investigator to resolve).
    assert!(
        session.state.current_mulligan().is_none(),
        "mulligan queue must be empty after the single investigator resolves"
    );

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actions WHERE game_id = ?")
        .bind("g2")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn apply_rejects_invalid_action_without_persisting() {
    let pool = common::memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g3",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .unwrap();

    // Post-create the mulligan is pending. Selecting a non-existent hand
    // index (OptionId(999_999)) is rejected by the mulligan handler since
    // the deck is empty (hand size 0, so any index ≥ 0 is out of bounds).
    let (events, outcome) = session
        .apply(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(999_999)],
            },
        })
        .await
        .unwrap();

    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "selecting a non-existent card must be rejected, got {outcome:?}"
    );
    assert!(events.is_empty());
    // State must be unchanged: round is still 1 and investigator still in
    // the mulligan queue.
    assert_eq!(
        session.state.round, 1,
        "state must be unchanged on rejection"
    );

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actions WHERE game_id = ?")
        .bind("g3")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "rejected action must not be persisted");
}

#[tokio::test]
async fn load_replays_log_to_reproduce_live_state() {
    let pool = common::memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g4",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .unwrap();
    // Resolve the mulligan so the log has one action.
    session
        .apply(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })
        .await
        .unwrap();

    let loaded = GameSession::load(pool.clone(), common::REGISTRIES, &GameId::new("g4"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.state, session.state);
    assert_eq!(loaded.game_id, session.game_id);

    // An unknown game id loads as None.
    let missing = GameSession::load(pool, common::REGISTRIES, &GameId::new("nope"))
        .await
        .unwrap();
    assert!(missing.is_none());
}

/// A log the engine does not accept must fail the load as an unresumable game,
/// not produce a half-reconstructed session.
///
/// An engine change to what an action *means* can leave a row perfectly
/// deserializable and not replayable. Simulated here by turning the logged
/// mulligan answer into a `Confirm`: it still answers the same prompt, which
/// the engine rejects a `Confirm` for — the same observable a stale log
/// produces.
#[tokio::test]
async fn load_fails_loudly_on_a_log_it_cannot_replay() {
    let pool = common::memory_pool().await;
    let mut session = common::create_game(&pool, "stale").await;
    session
        .apply(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })
        .await
        .unwrap();
    common::set_logged(&pool, "stale", 0, "$.response", "Confirm").await;

    let Err(err) = GameSession::load(pool.clone(), common::REGISTRIES, &GameId::new("stale")).await
    else {
        panic!("a log that cannot be replayed must fail the load");
    };
    assert!(
        matches!(
            &err,
            SessionError::Unresumable {
                game_id,
                reason: UnresumableReason::Replay {
                    seq: 0,
                    reason: ReplayFailure::Rejected(_),
                },
            } if game_id.as_str() == "stale"
        ),
        "the failure must name the game and the offending action, got {err:?}",
    );
    assert_eq!(
        common::stored_rows(&pool, "stale").await,
        (1, 1),
        "an unresumable game's rows are kept",
    );
}

/// Regression for #467: every game gets a fresh RNG seed, so the setup
/// shuffle/draw order differs across games.
#[tokio::test]
async fn create_randomizes_the_setup_seed_per_game() {
    // The mock scenario's setup() pins a fixed
    // builder seed (42); create() must override it with host entropy, so two
    // games created from the same module hold distinct seeds.
    let pool = common::memory_pool().await;
    let one = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g1",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("create g1");
    let two = GameSession::create(
        pool,
        common::REGISTRIES,
        "g2",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("create g2");
    assert_ne!(
        one.state.rng.seed, two.state.rng.seed,
        "each created game must get a distinct random setup seed"
    );
}

#[tokio::test]
async fn create_enables_interactive_acknowledge() {
    let pool = common::memory_pool().await;
    let session = GameSession::create(
        pool,
        common::REGISTRIES,
        "ack",
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("create");
    assert!(
        session.state.interactive_acknowledge,
        "human-play sessions pause to acknowledge skill-test results (#478)"
    );
}
