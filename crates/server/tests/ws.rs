//! Websocket hub: connect → `Hello`, accepted submits broadcast
//! `Applied` to every connection, rejected submits return `Rejected`
//! to the sender only.

mod common;

use common::TEST_SCENARIO_ID;
use std::time::Duration;

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::scenario::ScenarioId;
use protocol::{ClientMessage, ServerMessage};
use server::session::GameSession;
use sqlx::SqlitePool;

async fn seed_game(pool: &SqlitePool, game_id: &str) {
    GameSession::create(
        pool.clone(),
        game_id,
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
    )
    .await
    .expect("seed game");
}

#[tokio::test]
async fn connect_receives_hello_with_current_state() {
    common::install_registry();
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-hello").await;
    let addr = common::spawn_server(pool).await;

    let mut ws = common::connect(addr, "g-hello").await;

    match common::recv(&mut ws).await {
        ServerMessage::Hello { state, outcome, .. } => {
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
    common::install_registry();
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-bcast").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-bcast").await;
    let mut b = common::connect(addr, "g-bcast").await;
    // Draining each Hello guarantees both connections have subscribed
    // before the submit, so neither misses the broadcast.
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    // Resolve the setup mulligan (keep the full hand — empty redraw).
    common::send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        },
    )
    .await;

    for ws in [&mut a, &mut b] {
        match common::recv(ws).await {
            ServerMessage::Applied {
                outcome, events, ..
            } => {
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
    common::install_registry();
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-reject").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-reject").await;
    let mut b = common::connect(addr, "g-reject").await;
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    // Post-create the mulligan is pending. Selecting a non-existent hand
    // index (OptionId(999_999)) is rejected by the mulligan handler.
    common::send(
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

    match common::recv(&mut a).await {
        ServerMessage::Rejected { .. } => {}
        other => panic!("expected Rejected, got {other:?}"),
    }

    // B sees nothing: rejections are not broadcast.
    let quiet = tokio::time::timeout(Duration::from_millis(200), common::recv(&mut b)).await;
    assert!(
        quiet.is_err(),
        "B must receive nothing for a rejected action"
    );
}
