//! Websocket hub: connect → `Hello`, accepted submits broadcast
//! `Applied` to every connection, rejected submits return `Rejected`
//! to the sender only.

use std::time::Duration;

use axum::extract::ws::close_code;
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::scenario::ScenarioId;
use protocol::{
    ClientMessage, ServerMessage, CLOSE_GAME_NOT_FOUND, CLOSE_SERVER_RESTARTING, CLOSE_UNRESUMABLE,
};
use server::session::GameSession;
use server::shutdown::Shutdown;
use sqlx::SqlitePool;
use tokio::time;

use crate::common::{self, TEST_SCENARIO_ID};

async fn seed_game(pool: &SqlitePool, game_id: &str) {
    GameSession::create(
        pool.clone(),
        common::REGISTRIES,
        game_id,
        ScenarioId::new(TEST_SCENARIO_ID),
        common::roster(),
        0,
    )
    .await
    .expect("seed game");
}

#[tokio::test]
async fn connect_receives_hello_with_current_state() {
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
async fn connect_to_a_missing_game_is_closed_as_not_found_and_a_real_game_still_attaches() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-real").await;
    let addr = common::spawn_server(pool).await;

    let mut missing = common::connect(addr, "g-missing").await;
    common::assert_closed_with(&mut missing, CLOSE_GAME_NOT_FOUND).await;

    let mut real = common::connect(addr, "g-real").await;
    match common::recv(&mut real).await {
        ServerMessage::Hello { .. } => {}
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn connect_to_an_unresumable_game_is_closed_as_unresumable() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-unresumable").await;
    // A stored format version this build doesn't accept.
    common::set_format_version(&pool, "g-unresumable", 99).await;
    let addr = common::spawn_server(pool).await;

    let mut ws = common::connect(addr, "g-unresumable").await;
    common::assert_closed_with(&mut ws, CLOSE_UNRESUMABLE).await;
}

#[tokio::test]
async fn accepted_action_broadcasts_applied_to_all_clients() {
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
    let quiet = time::timeout(Duration::from_millis(200), common::recv(&mut b)).await;
    assert!(
        quiet.is_err(),
        "B must receive nothing for a rejected action"
    );
}

#[tokio::test]
async fn a_ping_is_answered_with_a_pong_to_that_socket_only() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-ping").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-ping").await;
    let mut b = common::connect(addr, "g-ping").await;
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    common::send(&mut a, &ClientMessage::Ping).await;

    assert_pong(&mut a).await;
    let quiet = time::timeout(Duration::from_millis(200), common::recv(&mut b)).await;
    assert!(quiet.is_err(), "B must receive nothing for A's ping");
}

/// The silence timeout the liveness tests inject in place of
/// [`protocol::SERVER_SILENCE_TIMEOUT`].
const SHORT_SILENCE: Duration = Duration::from_millis(300);

/// Require the next message on `ws` to be a `Pong`, within a bound so a missing
/// one fails the test rather than hanging it.
async fn assert_pong(ws: &mut common::Client) {
    match common::recv_within(ws).await {
        ServerMessage::Pong => {}
        other => panic!("expected Pong, got {other:?}"),
    }
}

#[tokio::test]
async fn a_socket_that_sends_nothing_is_closed_as_going_away() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-silent").await;
    let addr =
        common::spawn_server_with(pool, |state| state.with_silence_timeout(SHORT_SILENCE)).await;

    let mut ws = common::connect(addr, "g-silent").await;
    let _ = common::recv(&mut ws).await;

    common::assert_closed_with(&mut ws, close_code::AWAY).await;
}

#[tokio::test]
async fn a_socket_that_keeps_pinging_stays_open_past_the_timeout() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-pinging").await;
    let addr =
        common::spawn_server_with(pool, |state| state.with_silence_timeout(SHORT_SILENCE)).await;

    let mut ws = common::connect(addr, "g-pinging").await;
    let _ = common::recv(&mut ws).await;

    // Four pings half a timeout apart span twice the timeout, and each one is
    // still answered.
    for _ in 0..4 {
        time::sleep(SHORT_SILENCE / 2).await;
        common::send(&mut ws, &ClientMessage::Ping).await;
        assert_pong(&mut ws).await;
    }
}

#[tokio::test]
async fn the_shutdown_trigger_closes_every_open_socket_as_restarting() {
    let pool = common::memory_pool().await;
    seed_game(&pool, "g-one").await;
    seed_game(&pool, "g-two").await;
    let shutdown = Shutdown::new();
    let trigger = shutdown.clone();
    let addr = common::spawn_server_with(pool, |state| state.with_shutdown(trigger)).await;

    // Two sockets on one game and one on another, each past its `Hello`.
    let mut sockets = Vec::new();
    for game_id in ["g-one", "g-one", "g-two"] {
        let mut ws = common::connect(addr, game_id).await;
        let _ = common::recv_within(&mut ws).await;
        sockets.push(ws);
    }

    shutdown.trigger();

    for ws in &mut sockets {
        common::assert_closed_with(ws, CLOSE_SERVER_RESTARTING).await;
    }
}
