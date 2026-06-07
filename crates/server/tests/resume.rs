//! Reconnect + restart resume: a game paused at an in-flight
//! `AwaitingInput` is delivered to a reconnecting client (and rebuilt
//! from the log after a restart), and can be resolved over the wire.
//!
//! These are acceptance tests: the mechanics already fall out of the
//! generic outcome handling built in P5.2 (`load` reconstructs the
//! outcome by replay) and P5.3 (`Hello`/`Applied` carry the outcome,
//! `ResolveInput` is just another `Submit`). No new server code is
//! needed — these prove the pieces compose.

mod common;

use common::{connect, install_registry, memory_pool, recv, send, spawn_server, TEST_SCENARIO_ID};
use game_core::scenario::ScenarioId;
use game_core::state::{InvestigatorId, SkillKind};
use game_core::{EngineOutcome, InputResponse, PlayerAction};
use server::wire::{ClientMessage, ServerMessage};
use server::GameSession;

/// A skill test pauses at its commit window → `AwaitingInput`.
fn skill_test() -> ClientMessage {
    ClientMessage::Submit {
        action: PlayerAction::PerformSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            difficulty: 2,
        },
    }
}

async fn seed(pool: &sqlx::SqlitePool, game_id: &str) {
    GameSession::create(pool.clone(), game_id, ScenarioId::new(TEST_SCENARIO_ID))
        .await
        .expect("seed game");
}

#[tokio::test]
async fn reconnect_delivers_in_flight_awaiting_input() {
    install_registry();
    let pool = memory_pool().await;
    seed(&pool, "g-resume").await;
    let addr = spawn_server(pool).await;

    // Drive the game to AwaitingInput on connection A.
    let mut a = connect(addr, "g-resume").await;
    let _ = recv(&mut a).await; // Hello
    send(&mut a, &skill_test()).await;
    match recv(&mut a).await {
        ServerMessage::Applied { outcome, .. } => {
            assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        }
        other => panic!("expected Applied(AwaitingInput), got {other:?}"),
    }

    // A fresh connection sees the in-flight prompt in its Hello.
    let mut b = connect(addr, "g-resume").await;
    match recv(&mut b).await {
        ServerMessage::Hello { outcome, .. } => {
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "reconnect must surface the in-flight AwaitingInput, got {outcome:?}"
            );
        }
        other => panic!("expected Hello(AwaitingInput), got {other:?}"),
    }
}

#[tokio::test]
async fn restart_rebuilds_awaiting_input_via_replay() {
    install_registry();
    let pool = memory_pool().await;
    seed(&pool, "g-restart").await;

    // First server: drive to AwaitingInput.
    let addr1 = spawn_server(pool.clone()).await;
    let mut a = connect(addr1, "g-restart").await;
    let _ = recv(&mut a).await; // Hello
    send(&mut a, &skill_test()).await;
    let _ = recv(&mut a).await; // Applied(AwaitingInput)

    // "Restart": a fresh server with empty rooms over the same database.
    // The game is gone from memory and must be rebuilt by replay.
    let addr2 = spawn_server(pool).await;
    let mut c = connect(addr2, "g-restart").await;
    match recv(&mut c).await {
        ServerMessage::Hello { outcome, .. } => {
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "restart must rebuild AwaitingInput from the log, got {outcome:?}"
            );
        }
        other => panic!("expected Hello(AwaitingInput), got {other:?}"),
    }
}

#[tokio::test]
async fn resolve_input_resumes_and_completes() {
    install_registry();
    let pool = memory_pool().await;
    seed(&pool, "g-do-resolve").await;
    let addr = spawn_server(pool).await;

    let mut a = connect(addr, "g-do-resolve").await;
    let _ = recv(&mut a).await; // Hello
    send(&mut a, &skill_test()).await;
    let _ = recv(&mut a).await; // Applied(AwaitingInput)

    // Resolve: commit nothing. The engine finishes the test.
    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::CommitCards { indices: vec![] },
            },
        },
    )
    .await;
    match recv(&mut a).await {
        ServerMessage::Applied { outcome, .. } => {
            assert!(
                !matches!(
                    outcome,
                    EngineOutcome::AwaitingInput { .. } | EngineOutcome::Rejected { .. }
                ),
                "resolving the commit window completes the test, got {outcome:?}"
            );
        }
        other => panic!("expected Applied(completed), got {other:?}"),
    }
}

#[tokio::test]
async fn non_resolve_action_while_awaiting_input_is_rejected() {
    install_registry();
    let pool = memory_pool().await;
    seed(&pool, "g-busy").await;
    let addr = spawn_server(pool).await;

    let mut a = connect(addr, "g-busy").await;
    let _ = recv(&mut a).await; // Hello
    send(&mut a, &skill_test()).await;
    let _ = recv(&mut a).await; // Applied(AwaitingInput)

    // A non-ResolveInput submit while paused is rejected by the engine.
    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::EndTurn,
        },
    )
    .await;
    match recv(&mut a).await {
        ServerMessage::Rejected { .. } => {}
        other => panic!("expected Rejected while awaiting input, got {other:?}"),
    }
}
