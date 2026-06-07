//! `GameSession` persistence: create-from-setup, apply-and-persist,
//! load-by-replay. Driven against an in-memory `SQLite` with a mock
//! scenario registry installed (a fresh round-0 state, so
//! `StartScenario` is accepted and `EndTurn` is rejected).

use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};
use game_core::state::{GameState, InvestigatorId};
use game_core::test_support::builder::TestGame;
use game_core::test_support::fixtures::test_investigator;
use game_core::{EngineOutcome, Event, PlayerAction, Resolution};
use server::session::GameSession;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

const TEST_SCENARIO_ID: &str = "test-scenario";

fn test_setup() -> GameState {
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(TEST_SCENARIO_ID))
        .with_rng_seed(42)
        .build()
}

fn noop_resolution(_: &Resolution, _: &mut GameState, _: &mut Vec<Event>) {}

static TEST_MODULE: ScenarioModule = ScenarioModule {
    setup: test_setup,
    apply_resolution: noop_resolution,
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == TEST_SCENARIO_ID).then_some(&TEST_MODULE)
}

/// Install the mock registry. Idempotent: the integration tests in this
/// file share one process, so a second install is a harmless no-op.
fn install_registry() {
    let _ = game_core::scenario_registry::install(ScenarioRegistry { module_for });
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

#[tokio::test]
async fn create_persists_seed_and_exposes_setup_state() {
    install_registry();
    let pool = memory_pool().await;

    let session = GameSession::create(pool.clone(), "game-1", ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .expect("create session");

    // The in-memory state equals the scenario's setup() output.
    assert_eq!(session.state, test_setup());
    assert_eq!(session.game_id, "game-1");

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

    let result = GameSession::create(pool, "game-x", ScenarioId::new("no-such-scenario")).await;

    assert!(matches!(
        result,
        Err(server::session::SessionError::UnknownScenario(_))
    ));
}

#[tokio::test]
async fn apply_persists_accepted_action_and_advances_state() {
    install_registry();
    let pool = memory_pool().await;
    let mut session = GameSession::create(pool.clone(), "g2", ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .unwrap();

    let (_events, outcome) = session.apply(PlayerAction::StartScenario).await.unwrap();

    assert!(!matches!(outcome, EngineOutcome::Rejected { .. }));
    // StartScenario moves the round from 0 to 1.
    assert_eq!(session.state.round, 1);

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
    let mut session = GameSession::create(pool.clone(), "g3", ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .unwrap();

    // EndTurn is invalid from the round-0 Mythos setup state.
    let (events, outcome) = session.apply(PlayerAction::EndTurn).await.unwrap();

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(events.is_empty());
    assert_eq!(
        session.state.round, 0,
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
    let mut session = GameSession::create(pool.clone(), "g4", ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .unwrap();
    session.apply(PlayerAction::StartScenario).await.unwrap();

    let loaded = GameSession::load(pool.clone(), "g4")
        .await
        .unwrap()
        .expect("game exists");

    assert_eq!(loaded.state, session.state);
    assert_eq!(loaded.game_id, session.game_id);

    // An unknown game id loads as None.
    let missing = GameSession::load(pool, "nope").await.unwrap();
    assert!(missing.is_none());
}
