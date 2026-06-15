//! Browser transport (wasm only): bootstrap a game, connect its
//! WebSocket, fold inbound frames into the store, forward outbound
//! actions, and reconnect on close.

use futures::channel::mpsc;
use futures::{select, SinkExt, StreamExt};
use gloo_net::http::Request;
use gloo_net::websocket::{futures::WebSocket, Message};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use protocol::{ClientMessage, CreateGameRequest, CreateGameResponse, GameId, ServerMessage};

use crate::store::{reduce, ConnStatus, StoreSignal};
use crate::url::current_ws_url;

/// localStorage key holding the active game id across reloads.
const GAME_ID_KEY: &str = "eldritch_game_id";
/// The scenario the client requests on `POST /games` — The Gathering, now
/// that the server installs the real `cards`/`scenarios` registries (C7a).
const SCENARIO_ID: &str = "the-gathering";
/// Fixed reconnect backoff. Plenty for a solo v0; not exponential.
const RECONNECT_MS: u32 = 1000;

/// Sender used by views (the debug button, later P6.7 controls) to
/// submit actions; cloneable, survives reconnects.
pub type OutboundTx = mpsc::UnboundedSender<ClientMessage>;

/// Start the transport: provide an `OutboundTx` into context, then spawn
/// the bootstrap + connect loop. Call once from `App` (wasm only).
pub fn start(store: StoreSignal) {
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    provide_context(tx);
    spawn_local(run(store, rx));
}

async fn run(store: StoreSignal, mut rx: mpsc::UnboundedReceiver<ClientMessage>) {
    let mut game_id: GameId = match bootstrap(store).await {
        Some(id) => id,
        None => return, // bootstrap set ConnStatus::Failed already
    };

    loop {
        match connect_once(&store, &game_id, &mut rx).await {
            // Opened, but the server closed before any Hello: a valid game
            // always sends Hello immediately, so the id is unknown to the
            // server (e.g. a DB reset). Discard it and create a fresh game.
            ConnectOutcome::StaleId => {
                clear_saved_id();
                match create_game(store).await {
                    Some(id) => game_id = id,
                    None => return,
                }
            }
            // Couldn't reach the server, or a normal disconnect after a
            // live session — keep the SAME id and just retry. The server
            // may be restarting; recreating here would abandon the game
            // (and wipe the saved id) on every transient outage.
            ConnectOutcome::Unreachable | ConnectOutcome::Disconnected => {}
        }

        store.update(|s| s.status = ConnStatus::Reconnecting);
        TimeoutFuture::new(RECONNECT_MS).await;
    }
}

/// Outcome of one socket lifetime — the reconnect loop treats these three
/// cases differently (only `StaleId` discards the saved game id).
enum ConnectOutcome {
    /// `WebSocket::open` failed: the server is unreachable (down or
    /// restarting). Keep the id and retry.
    Unreachable,
    /// Opened, but the server closed before sending any `Hello` — the id
    /// is stale (unknown to the server). Discard it and recreate.
    StaleId,
    /// Connected, saw `Hello`, then the socket closed: a normal
    /// disconnect. Keep the id and reconnect.
    Disconnected,
}

/// Resolve a game id: reuse a saved one, else create and save. Returns
/// `None` (and sets `Failed`) if creation fails.
async fn bootstrap(store: StoreSignal) -> Option<GameId> {
    if let Some(id) = saved_id() {
        return Some(id);
    }
    create_game(store).await
}

async fn create_game(store: StoreSignal) -> Option<GameId> {
    let resp = Request::post("/games")
        .json(&CreateGameRequest {
            scenario_id: SCENARIO_ID.to_string(),
        })
        .ok()?
        .send()
        .await
        .ok()?;
    if let Ok(r) = resp.json::<CreateGameResponse>().await {
        save_id(&r.game_id);
        Some(r.game_id)
    } else {
        store.update(|s| s.status = ConnStatus::Failed);
        None
    }
}

/// One socket lifetime: open, mark Connected, pump inbound->reduce and
/// outbound->sink until close. The returned [`ConnectOutcome`] tells the
/// reconnect loop whether the id is stale, the server is unreachable, or
/// it was a normal disconnect.
async fn connect_once(
    store: &StoreSignal,
    game_id: &GameId,
    rx: &mut mpsc::UnboundedReceiver<ClientMessage>,
) -> ConnectOutcome {
    store.update(|s| s.status = ConnStatus::Connecting);
    let Ok(ws) = WebSocket::open(&current_ws_url(game_id.as_str())) else {
        return ConnectOutcome::Unreachable;
    };
    store.update(|s| s.status = ConnStatus::Connected);

    let (mut write, read) = ws.split();
    let mut read = read.fuse();
    let mut saw_hello = false;

    loop {
        select! {
            incoming = read.next() => match incoming {
                Some(Ok(Message::Text(txt))) => {
                    if let Ok(msg) = serde_json::from_str::<ServerMessage>(&txt) {
                        saw_hello |= matches!(msg, ServerMessage::Hello { .. });
                        store.update(|s| reduce(s, msg));
                    }
                }
                Some(Ok(Message::Bytes(_))) => {}
                Some(Err(_)) | None => break, // socket closed/errored
            },
            outbound = rx.next() => {
                if let Some(action) = outbound {
                    if let Ok(json) = serde_json::to_string(&action) {
                        let _ = write.send(Message::Text(json)).await;
                    }
                }
            }
        }
    }
    if saw_hello {
        ConnectOutcome::Disconnected
    } else {
        ConnectOutcome::StaleId
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
fn saved_id() -> Option<GameId> {
    let raw = local_storage()?.get_item(GAME_ID_KEY).ok().flatten()?;
    Some(GameId::new(raw))
}
fn save_id(id: &GameId) {
    if let Some(ls) = local_storage() {
        let _ = ls.set_item(GAME_ID_KEY, id.as_str());
    }
}
fn clear_saved_id() {
    if let Some(ls) = local_storage() {
        let _ = ls.remove_item(GAME_ID_KEY);
    }
}
