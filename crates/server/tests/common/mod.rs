//! Shared helpers for server integration tests: a mock scenario
//! registry, an in-memory pool, a spawned server, and a websocket
//! client. Not every test binary uses every helper.
#![allow(dead_code)]

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};
use game_core::state::GameStateBuilder;
use game_core::state::{ChaosBag, ChaosToken, GameState, InvestigatorId};
use game_core::test_support::fixtures::test_investigator;
use game_core::{Event, Resolution};
use protocol::{ClientMessage, ServerMessage};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

pub const TEST_SCENARIO_ID: &str = "test-scenario";

fn test_setup() -> GameState {
    // Round-0 Mythos state: `StartScenario` is accepted, `EndTurn` is
    // rejected. The single-token chaos bag also makes the investigator
    // eligible for a `PerformSkillTest` (which pauses at a commit window
    // → `AwaitingInput`), exercised by the resume tests.
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_scenario_id(ScenarioId::new(TEST_SCENARIO_ID))
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

/// Install the mock scenario registry (idempotent within a process).
pub fn install_registry() {
    let _ = game_core::scenario_registry::install(ScenarioRegistry { module_for });
}

/// A migrated single-connection in-memory pool.
pub async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    server::db::MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

/// Spawn the server on an ephemeral port; return its bound address.
pub async fn spawn_server(pool: SqlitePool) -> SocketAddr {
    let app = server::app(server::AppState::new(pool));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

pub type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Open a websocket to a game.
pub async fn connect(addr: SocketAddr, game_id: &str) -> Client {
    let url = format!("ws://{addr}/ws/{game_id}");
    let (ws, _response) = connect_async(url).await.expect("ws connect");
    ws
}

/// Send a client message as JSON text.
pub async fn send(ws: &mut Client, msg: &ClientMessage) {
    let json = serde_json::to_string(msg).expect("serialize ClientMessage");
    ws.send(Message::Text(json.into())).await.expect("ws send");
}

/// Receive the next [`ServerMessage`], skipping ping/pong frames.
pub async fn recv(ws: &mut Client) -> ServerMessage {
    loop {
        match ws.next().await.expect("stream open").expect("no ws error") {
            Message::Text(text) => {
                return serde_json::from_str(text.as_str()).expect("valid ServerMessage");
            }
            Message::Close(_) => panic!("server closed the connection unexpectedly"),
            _ => {}
        }
    }
}
