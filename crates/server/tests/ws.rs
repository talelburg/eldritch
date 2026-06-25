//! Websocket hub: connect → `Hello`, accepted submits broadcast
//! `Applied` to every connection, rejected submits return `Rejected`
//! to the sender only.

mod common;

use common::{connect, install_registry, memory_pool, recv, send, spawn_server, TEST_SCENARIO_ID};
use game_core::scenario::ScenarioId;
use game_core::state::InvestigatorId;
use game_core::{EngineOutcome, InputResponse, OptionId, PlayerAction};
use protocol::{ClientMessage, ServerMessage};

async fn seed_game(pool: &sqlx::SqlitePool, game_id: &str) {
    server::GameSession::create(pool.clone(), game_id, ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .expect("seed game");
}

#[tokio::test]
async fn connect_receives_hello_with_current_state() {
    install_registry();
    let pool = memory_pool().await;
    seed_game(&pool, "g-hello").await;
    let addr = spawn_server(pool).await;

    let mut ws = connect(addr, "g-hello").await;

    match recv(&mut ws).await {
        ServerMessage::Hello { state, outcome } => {
            assert_eq!(state.round, 0);
            assert!(matches!(outcome, EngineOutcome::Done));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn accepted_action_broadcasts_applied_to_all_clients() {
    install_registry();
    let pool = memory_pool().await;
    seed_game(&pool, "g-bcast").await;
    let addr = spawn_server(pool).await;

    let mut a = connect(addr, "g-bcast").await;
    let mut b = connect(addr, "g-bcast").await;
    // Draining each Hello guarantees both connections have subscribed
    // before the submit, so neither misses the broadcast.
    let _ = recv(&mut a).await;
    let _ = recv(&mut b).await;

    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::StartScenario { roster: vec![] },
        },
    )
    .await;

    for ws in [&mut a, &mut b] {
        match recv(ws).await {
            ServerMessage::Applied {
                state,
                events,
                outcome,
            } => {
                assert!(!matches!(outcome, EngineOutcome::Rejected { .. }));
                assert!(!events.is_empty(), "StartScenario emits events");
                // The broadcast carries the authoritative post-action
                // state: StartScenario opens the setup mulligan loop, so the
                // first investigator is prompted (it was None in the
                // pre-action Hello).
                assert_eq!(state.current_mulligan(), Some(InvestigatorId(1)));
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn rejected_action_returns_rejected_to_sender_only() {
    install_registry();
    let pool = memory_pool().await;
    seed_game(&pool, "g-reject").await;
    let addr = spawn_server(pool).await;

    let mut a = connect(addr, "g-reject").await;
    let mut b = connect(addr, "g-reject").await;
    let _ = recv(&mut a).await;
    let _ = recv(&mut b).await;

    // A `ResolveInput` with no outstanding prompt is invalid from the
    // round-0 setup state (stands in for the removed typed gameplay variants).
    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(0)),
            },
        },
    )
    .await;

    match recv(&mut a).await {
        ServerMessage::Rejected { reason } => {
            assert!(
                reason.contains("no AwaitingInput prompt"),
                "reason was: {reason}"
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    // B sees nothing: rejections are not broadcast.
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(200), recv(&mut b)).await;
    assert!(
        quiet.is_err(),
        "B must receive nothing for a rejected action"
    );
}
