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
use protocol::live::Refused;
use protocol::lobby::Start;
use protocol::logged_answer::ReplayFailure;
use protocol::{Assignment, GameId};
use server::session::{ApplyError, GameSession, SessionError, UnresumableReason};

use crate::common::{self, TEST_SCENARIO_ID};

/// Regression test for the load bug: a game with zero logged actions, whose
/// setup ends `AwaitingInput` at the mulligan, must load as `AwaitingInput`,
/// not `Done`.
#[tokio::test]
async fn load_restores_awaiting_input_seed_with_empty_log() {
    let pool = common::memory_pool().await;
    let creator = common::test_account(&pool).await;
    let session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "fresh",
        creator,
        common::solo_start(creator),
    )
    .await
    .expect("create");
    // create seats at creation → mulligan-pending, no actions logged yet.
    assert!(matches!(
        session.live().outcome(),
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(session
        .live()
        .state()
        .investigators
        .contains_key(&InvestigatorId(1)));

    let loaded = GameSession::load(pool, common::REGISTRIES, &GameId::new("fresh"))
        .await
        .unwrap()
        .expect("exists");
    assert_eq!(
        loaded.live().state(),
        session.live().state(),
        "load reproduces the state setup left"
    );
    assert!(
        matches!(loaded.live().outcome(), EngineOutcome::AwaitingInput { .. }),
        "load must restore setup's AwaitingInput outcome from an empty log, got {:?}",
        loaded.live().outcome()
    );
}

#[tokio::test]
async fn create_persists_seed_and_exposes_setup_state() {
    let pool = common::memory_pool().await;

    let creator = common::test_account(&pool).await;
    let session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "game-1",
        creator,
        common::solo_start(creator),
    )
    .await
    .expect("create session");

    // create seats the roster: the investigator is present and round is 1.
    assert!(session
        .live()
        .state()
        .investigators
        .contains_key(&InvestigatorId(1)));
    assert_eq!(session.live().state().round, 1);
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

    let creator = common::test_account(&pool).await;
    let result = GameSession::create(
        pool,
        common::REGISTRIES,
        "game-x",
        creator,
        Start {
            scenario_id: ScenarioId::new("no-such-scenario"),
            roster: vec![],
            lead: 0,
            assignment: common::assign_all(creator, &[]),
        },
    )
    .await;

    assert!(matches!(result, Err(SessionError::UnknownScenario(_))));
}

/// Every seated investigator needs exactly one holder, or nobody could answer
/// its prompts: an assignment naming a seat the roster doesn't fill is
/// refused, and nothing is stored.
#[tokio::test]
async fn create_refuses_an_assignment_that_is_not_the_seated_investigators() {
    let pool = common::memory_pool().await;
    let creator = common::test_account(&pool).await;

    let result = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "mismatched",
        creator,
        Start {
            scenario_id: ScenarioId::new(TEST_SCENARIO_ID),
            roster: common::roster(),
            lead: 0,
            assignment: Assignment::in_seating_order([creator, creator]),
        },
    )
    .await;

    assert!(
        matches!(result, Err(SessionError::AssignmentMismatch { .. })),
        "a one-seat roster assigned two seats must be refused"
    );
    assert_eq!(common::stored_rows(&pool, "mismatched").await, (0, 0));
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

    let creator = common::test_account(&pool).await;
    let assignment = common::assign_all(creator, &bad_roster);
    let result = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "bad",
        creator,
        Start {
            scenario_id: ScenarioId::new(TEST_SCENARIO_ID),
            roster: bad_roster,
            lead: 0,
            assignment: assignment,
        },
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
    let creator = common::test_account(&pool).await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g2",
        creator,
        common::solo_start(creator),
    )
    .await
    .unwrap();

    // create lands at round-1 mulligan-pending; resolve the mulligan (keep
    // the whole hand — empty redraw).
    session
        .apply(
            common::test_account(&pool).await,
            session.live().prompt(),
            PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        )
        .await
        .expect("the mulligan is accepted");
    // After the mulligan resolves the mulligan queue must be empty (solo
    // game: only one investigator to resolve).
    assert!(
        session.live().state().current_mulligan().is_none(),
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
    let creator = common::test_account(&pool).await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g3",
        creator,
        common::solo_start(creator),
    )
    .await
    .unwrap();

    // Post-create the mulligan is pending. Selecting a non-existent hand
    // index (OptionId(999_999)) is rejected by the mulligan handler since
    // the deck is empty (hand size 0, so any index ≥ 0 is out of bounds).
    let refused = session
        .apply(
            common::test_account(&pool).await,
            session.live().prompt(),
            PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple {
                    selected: vec![OptionId(999_999)],
                },
            },
        )
        .await;

    assert!(
        matches!(refused, Err(ApplyError::Refused(Refused::Engine(_)))),
        "selecting a non-existent card must be rejected, got {refused:?}"
    );
    // State must be unchanged: round is still 1 and investigator still in
    // the mulligan queue.
    assert_eq!(
        session.live().state().round,
        1,
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
    let creator = common::test_account(&pool).await;
    let mut session = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g4",
        creator,
        common::solo_start(creator),
    )
    .await
    .unwrap();
    // Resolve the mulligan so the log has one action.
    session
        .apply(
            common::test_account(&pool).await,
            session.live().prompt(),
            PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        )
        .await
        .expect("the mulligan is accepted");

    let loaded = GameSession::load(pool.clone(), common::REGISTRIES, &GameId::new("g4"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.live().state(), session.live().state());
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
        .apply(
            common::test_account(&pool).await,
            session.live().prompt(),
            PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        )
        .await
        .expect("the mulligan is accepted");
    common::set_logged(&pool, "stale", 0, "$.response", "Confirm").await;

    let Err(err) = GameSession::load(pool.clone(), common::REGISTRIES, &GameId::new("stale")).await
    else {
        panic!("a log that cannot be replayed must fail the load");
    };
    assert!(
        matches!(
            &err,
            SessionError::Unresumable { game_id, reason }
                if game_id.as_str() == "stale"
                    && matches!(
                        **reason,
                        UnresumableReason::Replay {
                            seq: 0,
                            reason: ReplayFailure::Rejected(_),
                        }
                    )
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
    let creator = common::test_account(&pool).await;
    let one = GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        "g1",
        creator,
        common::solo_start(creator),
    )
    .await
    .expect("create g1");
    let creator = common::test_account(&pool).await;
    let two = GameSession::create(
        pool,
        common::REGISTRIES,
        "g2",
        creator,
        common::solo_start(creator),
    )
    .await
    .expect("create g2");
    assert_ne!(
        one.live().state().rng.seed,
        two.live().state().rng.seed,
        "each created game must get a distinct random setup seed"
    );
}

#[tokio::test]
async fn create_enables_interactive_acknowledge() {
    let pool = common::memory_pool().await;
    let creator = common::test_account(&pool).await;
    let session = GameSession::create(
        pool,
        common::REGISTRIES,
        "ack",
        creator,
        common::solo_start(creator),
    )
    .await
    .expect("create");
    assert!(
        session.live().state().interactive_acknowledge,
        "human-play sessions pause to acknowledge skill-test results (#478)"
    );
}
