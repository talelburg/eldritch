//! Websocket hub: connect → `Hello`, accepted submits broadcast
//! `Applied` to every connection, rejected submits return `Rejected`
//! to the sender only.

mod common;

use common::{connect, install_registry, memory_pool, recv, roster, send, spawn_server,
             TEST_SCENARIO_ID};
use game_core::scenario::ScenarioId;
use game_core::{EngineOutcome, InputResponse, OptionId, PlayerAction};
use protocol::{ClientMessage, ServerMessage};

async fn seed_game(pool: &sqlx::SqlitePool, game_id: &str) {
    server::GameSession::create(
        pool.clone(),
        game_id,
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
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
            // create seats the roster: round is 1, mulligan is pending.
            assert_eq!(state.round, 1);
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "freshly-created game is mulligan-pending, got {outcome:?}"
            );
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

    // Resolve the setup mulligan (keep the full hand — empty redraw).
    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        },
    )
    .await;

    for ws in [&mut a, &mut b] {
        match recv(ws).await {
            ServerMessage::Applied { outcome, events, .. } => {
                assert!(!matches!(outcome, EngineOutcome::Rejected { .. }));
                assert!(
                    matches!(outcome, EngineOutcome::AwaitingInput { .. } | EngineOutcome::Done),
                    "resolving the mulligan advances to the open-turn menu or completes, got {outcome:?}"
                );
                assert!(!events.is_empty(), "resolving the mulligan emits events");
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

    // Post-create the mulligan is pending. Selecting a non-existent hand
    // index (OptionId(999_999)) is rejected by the mulligan handler.
    send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple {
                    selected: vec![OptionId(999_999)],
                },
            },
        },
    )
    .await;

    match recv(&mut a).await {
        ServerMessage::Rejected { .. } => {}
        other => panic!("expected Rejected, got {other:?}"),
    }

    // B sees nothing: rejections are not broadcast.
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(200), recv(&mut b)).await;
    assert!(
        quiet.is_err(),
        "B must receive nothing for a rejected action"
    );
}
