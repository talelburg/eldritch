//! Reconnect + restart resume: a game paused at an in-flight
//! `AwaitingInput` is delivered to a reconnecting client (and rebuilt
//! from the persisted seed outcome after a restart), and can be resolved
//! over the wire.
//!
//! These are acceptance tests: the mechanics fall out of the generic
//! outcome handling built in P5.2 (`load` restores the seed outcome,
//! replayed actions overwrite) and P5.3 (`Hello`/`Applied` carry the
//! outcome, `ResolveInput` is just another `Submit`). No new server code
//! is needed — these prove the pieces compose.

mod common;

use common::TEST_SCENARIO_ID;
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::scenario::ScenarioId;
use protocol::{ClientMessage, ServerMessage};
use server::session::GameSession;
use sqlx::SqlitePool;

async fn seed(pool: &SqlitePool, game_id: &str) {
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
async fn reconnect_delivers_in_flight_awaiting_input() {
    common::install_registry();
    let pool = common::memory_pool().await;
    seed(&pool, "g-resume").await;
    let addr = common::spawn_server(pool).await;

    // Connection A's first Hello is already AwaitingInput (the mulligan).
    let mut a = common::connect(addr, "g-resume").await;
    match common::recv(&mut a).await {
        ServerMessage::Hello { outcome, .. } => {
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "first Hello must be mulligan-pending, got {outcome:?}"
            );
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // A fresh connection also sees the in-flight prompt in its Hello.
    let mut b = common::connect(addr, "g-resume").await;
    match common::recv(&mut b).await {
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
async fn restart_restores_awaiting_input_from_persisted_seed_outcome() {
    common::install_registry();
    let pool = common::memory_pool().await;
    seed(&pool, "g-restart").await;

    // "Restart": a fresh server with empty rooms over the same database.
    // The game has zero logged actions; AwaitingInput is restored from the
    // persisted seed_outcome (not from log replay).
    let addr = common::spawn_server(pool).await;
    let mut c = common::connect(addr, "g-restart").await;
    match common::recv(&mut c).await {
        ServerMessage::Hello { outcome, .. } => {
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "restart must restore AwaitingInput from the seed outcome, got {outcome:?}"
            );
        }
        other => panic!("expected Hello(AwaitingInput), got {other:?}"),
    }
}

#[tokio::test]
async fn resolve_input_resumes_and_completes() {
    common::install_registry();
    let pool = common::memory_pool().await;
    seed(&pool, "g-do-resolve").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-do-resolve").await;
    let _ = common::recv(&mut a).await; // Hello (AwaitingInput — mulligan already pending)

    // Resolve: keep the whole hand (empty redraw). The mulligan completes and
    // the engine drives forward into the Investigation phase, surfacing the
    // open-turn action menu (`AwaitingInput`) — i.e. the resolve is accepted and
    // makes progress, not rejected.
    common::send(
        &mut a,
        &ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        },
    )
    .await;
    match common::recv(&mut a).await {
        ServerMessage::Applied { outcome, .. } => {
            assert!(
                !matches!(outcome, EngineOutcome::Rejected { .. }),
                "resolving the mulligan is accepted and drives forward, got {outcome:?}"
            );
        }
        other => panic!("expected Applied(completed), got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_mulligan_response_is_rejected() {
    common::install_registry();
    let pool = common::memory_pool().await;
    seed(&pool, "g-busy").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-busy").await;
    let _ = common::recv(&mut a).await; // Hello (AwaitingInput — mulligan already pending)

    // Submitting a ResolveInput with an out-of-range option against the
    // mulligan is rejected: OptionId(999_999) is out of bounds since the
    // deck is empty (hand size 0).
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
        other => panic!("expected Rejected while awaiting input, got {other:?}"),
    }
}
