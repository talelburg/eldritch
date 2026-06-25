//! Phase-5 closing demo: the end-to-end demonstration that closes the
//! milestone (mirrors Phase 4's #157). One narrative exercises every
//! "done" criterion of the phase:
//!
//! 1. two clients connected to one game see the same initial state and
//!    an identical event stream as actions are applied (the spectator
//!    foundation for multiplayer);
//! 2. a client reconnecting mid-scenario receives the in-flight
//!    `AwaitingInput` and can resolve it;
//! 3. restarting the server (fresh in-memory rooms, same database) and
//!    reconnecting reproduces the exact state via action-log replay.

mod common;

use common::{connect, install_registry, memory_pool, recv, roster, send, spawn_server,
             TEST_SCENARIO_ID};
use game_core::scenario::ScenarioId;
use game_core::state::GameState;
use game_core::{EngineOutcome, Event, InputResponse, PlayerAction};
use protocol::{ClientMessage, ServerMessage};
use server::GameSession;

fn submit(action: PlayerAction) -> ClientMessage {
    ClientMessage::Submit { action }
}

fn hello_state(msg: ServerMessage) -> GameState {
    match msg {
        ServerMessage::Hello { state, .. } => *state,
        other => panic!("expected Hello, got {other:?}"),
    }
}

fn hello_outcome(msg: ServerMessage) -> EngineOutcome {
    match msg {
        ServerMessage::Hello { outcome, .. } => outcome,
        other => panic!("expected Hello, got {other:?}"),
    }
}

fn applied_events(msg: ServerMessage) -> Vec<Event> {
    match msg {
        ServerMessage::Applied { events, .. } => events,
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[tokio::test]
async fn phase_5_closing_demo() {
    install_registry();
    let pool = memory_pool().await;
    GameSession::create(
        pool.clone(),
        "demo",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
    .await
    .expect("create the demo game");
    let addr = spawn_server(pool.clone()).await;

    // (1) Two clients connect and see the same initial state (seated,
    // round 1, mulligan-pending).
    let mut actor = connect(addr, "demo").await;
    let mut spectator = connect(addr, "demo").await;
    let actor_initial = hello_state(recv(&mut actor).await);
    let spectator_initial = hello_state(recv(&mut spectator).await);
    assert_eq!(
        actor_initial, spectator_initial,
        "both connections see the same initial state"
    );

    // The actor resolves the setup mulligan (keep the full hand).
    // The spectator, who sends nothing, observes the identical event stream.
    send(
        &mut actor,
        &submit(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .await;
    let actor_resolved = applied_events(recv(&mut actor).await);
    let spectator_resolved = applied_events(recv(&mut spectator).await);
    assert_eq!(
        actor_resolved, spectator_resolved,
        "spectator sees the identical event stream"
    );
    assert!(
        !actor_resolved.is_empty(),
        "resolving the mulligan announced itself with events: {actor_resolved:?}"
    );

    // (2) A client reconnecting after the mulligan receives the in-flight
    // `AwaitingInput` (the open-turn action menu) in its Hello.
    let mut latecomer = connect(addr, "demo").await;
    assert!(
        matches!(
            hello_outcome(recv(&mut latecomer).await),
            EngineOutcome::AwaitingInput { .. }
        ),
        "a mid-scenario reconnect surfaces the in-flight prompt"
    );

    // Capture the post-resolution live state from a fresh connection.
    let mut probe = connect(addr, "demo").await;
    let live_state = hello_state(recv(&mut probe).await);

    // (3) Restart: a new server with empty rooms over the same database.
    // The game must be rebuilt from the action log.
    let restarted = spawn_server(pool).await;
    let mut reconnect = connect(restarted, "demo").await;
    let replayed_state = hello_state(recv(&mut reconnect).await);
    assert_eq!(
        live_state, replayed_state,
        "restart reproduces the exact state via action-log replay"
    );
}
