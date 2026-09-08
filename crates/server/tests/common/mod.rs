//! Shared helpers for server integration tests: a mock scenario
//! registry, an in-memory pool, a spawned server, and a websocket
//! client. Not every test binary uses every helper.
#![allow(dead_code)]

use std::net::SocketAddr;

use futures_util::{SinkExt as _, StreamExt as _};
use game_core::action::RosterEntry;
use game_core::event::Event;
use game_core::scenario::{ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry};
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder};
use game_core::{scenario_registry, test_support};
use protocol::{ClientMessage, ServerMessage};
use server::db::MIGRATOR;
use server::AppState;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

pub const TEST_SCENARIO_ID: &str = "test-scenario";

fn test_setup() -> GameState {
    // Bare scenario seed — seating runs at creation via `seat_and_open`,
    // so the pre-seeded investigator and turn order are NOT injected here.
    GameStateBuilder::new()
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_scenario_id(ScenarioId::new(TEST_SCENARIO_ID))
        .with_rng_seed(42)
        .build()
}

fn noop_resolution(_: ScenarioEnding, _: &mut GameState, _: &mut Vec<Event>) {}

static TEST_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: test_setup,
    apply_resolution: noop_resolution,
    layout: &[],
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == TEST_SCENARIO_ID).then_some(&TEST_MODULE)
}

/// Install the mock scenario registry + the synthetic card registry
/// (idempotent: second install is a no-op).
pub fn install_registry() {
    let _ = scenario_registry::install(ScenarioRegistry { module_for });
    test_support::install_test_registry();
}

/// A one-investigator roster using the synthetic `TEST_INV` code.
pub fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }]
}

/// A migrated single-connection in-memory pool.
pub async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

/// Spawn the server on an ephemeral port; return its bound address.
pub async fn spawn_server(pool: SqlitePool) -> SocketAddr {
    let app = server::app(AppState::new(pool));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

pub type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

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
