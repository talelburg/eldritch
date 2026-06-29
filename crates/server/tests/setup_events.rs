//! #512: the events emitted during scenario setup (`seat_and_open`) are
//! persisted at create time and delivered in the reconnect baseline `Hello`,
//! so the client's event log can show the opening draws / shuffles / weakness
//! set-aside. This is the path the live client hits: `POST /games` creates and
//! persists the session, then the websocket connection reloads it from the DB
//! (the in-memory created session is not held), so the events must survive the
//! round-trip through the persisted column — not just live in memory.

mod common;

use common::{
    connect, install_registry, memory_pool, recv, roster, spawn_server, TEST_SCENARIO_ID,
};
use game_core::scenario::ScenarioId;
use protocol::ServerMessage;
use server::GameSession;

#[tokio::test]
async fn hello_carries_setup_events_after_reload_from_db() {
    install_registry();
    let pool = memory_pool().await;
    // Create + persist a game (as `POST /games` does), then spin a server that
    // serves it purely from the DB — proving the events come from the persisted
    // column, not a retained in-memory session.
    GameSession::create(
        pool.clone(),
        "g-setup-events",
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
    )
    .await
    .expect("seed game");
    let addr = spawn_server(pool).await;

    let mut c = connect(addr, "g-setup-events").await;
    match recv(&mut c).await {
        ServerMessage::Hello { events, .. } => {
            assert!(
                !events.is_empty(),
                "the reconnect Hello must carry the persisted setup events; \
                 got an empty list (setup events were dropped on reload)",
            );
            // ScenarioStarted is always emitted at setup, so it must be present.
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, game_core::Event::ScenarioStarted)),
                "setup events should include ScenarioStarted; got {events:?}",
            );
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}
