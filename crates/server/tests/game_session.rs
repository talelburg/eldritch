//! `GameSession` persistence: create-from-setup, apply-and-persist,
//! load-by-replay. Driven against an in-memory `SQLite` with a mock
//! scenario registry installed (seating runs at creation: the seed is
//! round-1 mulligan-pending, so a `ResolveInput(PickMultiple{selected:[]})` is
//! accepted and a `ResolveInput` selecting a non-existent card is rejected).

use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};
use game_core::state::GameStateBuilder;
use game_core::state::{ChaosBag, ChaosToken, GameState, InvestigatorId};
use game_core::{EngineOutcome, Event, InputResponse, OptionId, PlayerAction, Resolution};
use server::session::GameSession;
use server::GameId;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

const TEST_SCENARIO_ID: &str = "test-scenario";

fn test_setup() -> GameState {
    GameStateBuilder::new()
        .with_scenario_id(ScenarioId::new(TEST_SCENARIO_ID))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_rng_seed(42)
        .build()
}

fn noop_resolution(_: &Resolution, _: &mut GameState, _: &mut Vec<Event>) {}

static TEST_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: test_setup,
    apply_resolution: noop_resolution,
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == TEST_SCENARIO_ID).then_some(&TEST_MODULE)
}

/// Install the mock scenario registry + the synthetic card registry
/// (idempotent: within a process, second install is a harmless no-op).
fn install_registry() {
    let _ = game_core::scenario_registry::install(ScenarioRegistry { module_for });
    game_core::test_support::install_test_registry();
}

fn roster() -> Vec<game_core::action::RosterEntry> {
    vec![game_core::action::RosterEntry {
        investigator: game_core::state::CardCode::new(game_core::test_support::TEST_INV),
        deck: vec![],
    }]
}

async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    server::db::MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

/// Regression test for the load bug: a game with zero logged actions whose
/// seed outcome is `AwaitingInput` must load as `AwaitingInput`, not `Done`.
#[tokio::test]
async fn load_restores_awaiting_input_seed_with_empty_log() {
    install_registry();
    let pool = memory_pool().await;
    let session = GameSession::create(
        pool.clone(),
        "seeded",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
    .await
    .expect("create");
    // create seats at creation → mulligan-pending, no actions logged yet.
    assert!(matches!(
        session.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert!(session.state.investigators.contains_key(&InvestigatorId(1)));

    let loaded = GameSession::load(pool, &GameId::new("seeded"))
        .await
        .unwrap()
        .expect("exists");
    assert_eq!(
        loaded.state, session.state,
        "load reproduces the seeded state"
    );
    assert!(
        matches!(loaded.outcome, EngineOutcome::AwaitingInput { .. }),
        "load must restore the seed's AwaitingInput outcome from an empty log, got {:?}",
        loaded.outcome
    );
}

#[tokio::test]
async fn create_persists_seed_and_exposes_setup_state() {
    install_registry();
    let pool = memory_pool().await;

    let session = GameSession::create(
        pool.clone(),
        "game-1",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
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
    install_registry();
    let pool = memory_pool().await;

    let result =
        GameSession::create(pool, "game-x", ScenarioId::new("no-such-scenario"), vec![]).await;

    assert!(matches!(
        result,
        Err(server::session::SessionError::UnknownScenario(_))
    ));
}

#[tokio::test]
async fn create_rejects_bad_roster() {
    install_registry();
    let pool = memory_pool().await;

    // Use an obviously-unknown investigator code; the synthetic registry
    // resolves nothing for it, so seating rejects.
    let bad_roster = vec![game_core::action::RosterEntry {
        investigator: game_core::state::CardCode::new("99999"),
        deck: vec![],
    }];

    let result = GameSession::create(
        pool.clone(),
        "bad",
        ScenarioId::new(TEST_SCENARIO_ID),
        bad_roster,
    )
    .await;

    assert!(
        matches!(result, Err(server::session::SessionError::Seating(_))),
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
    install_registry();
    let pool = memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        "g2",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
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
    install_registry();
    let pool = memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        "g3",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
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
    install_registry();
    let pool = memory_pool().await;
    let mut session = GameSession::create(
        pool.clone(),
        "g4",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
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

    let loaded = GameSession::load(pool.clone(), &GameId::new("g4"))
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.state, session.state);
    assert_eq!(loaded.game_id, session.game_id);

    // An unknown game id loads as None.
    let missing = GameSession::load(pool, &GameId::new("nope")).await.unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn create_randomizes_the_setup_seed_per_game() {
    // #467: every game must get a fresh RNG seed so the setup shuffle/draw
    // order differs across games. The mock scenario's setup() pins a fixed
    // builder seed (42); create() must override it with host entropy, so two
    // games created from the same module hold distinct frozen seeds.
    install_registry();
    let pool = memory_pool().await;
    let one = GameSession::create(
        pool.clone(),
        "g1",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
    .await
    .expect("create g1");
    let two = GameSession::create(pool, "g2", ScenarioId::new(TEST_SCENARIO_ID), roster())
        .await
        .expect("create g2");
    assert_ne!(
        one.state.rng.seed, two.state.rng.seed,
        "each created game must get a distinct random setup seed"
    );
}

#[tokio::test]
async fn create_enables_interactive_acknowledge() {
    install_registry();
    let pool = memory_pool().await;
    let session = GameSession::create(pool, "ack", ScenarioId::new(TEST_SCENARIO_ID), roster())
        .await
        .expect("create");
    assert!(
        session.state.interactive_acknowledge,
        "human-play sessions pause to acknowledge skill-test results (#478)"
    );
}
