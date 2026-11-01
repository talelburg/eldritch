//! Browser transport (wasm only): check sign-in, bootstrap a game, connect
//! its WebSocket, fold inbound frames into the store, forward outbound
//! actions, and reconnect on close with backoff.

use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future::Fuse;
use futures::stream::SplitSink;
use futures::{select, FutureExt as _, SinkExt as _, StreamExt as _};
use gloo_net::http::Request;
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::{Message, WebSocketError};
use gloo_timers::future::{IntervalStream, TimeoutFuture};
use leptos::prelude::*;
use protocol::{
    ClientMessage, CreateGameRequest, CreateGameResponse, GameId, ServerMessage, SignInRequired,
    SignedIn,
};
use wasm_bindgen_futures::spawn_local;
use web_sys::Storage;

use crate::picker::CreateTx;
use crate::socket_ending::{self, SocketEnding};
use crate::store::{self, ConnStatus, StoreSignal};
use crate::url;

/// localStorage key holding the active game id across reloads.
const GAME_ID_KEY: &str = "eldritch_game_id";
/// HTTP 401, which the server answers when the request isn't signed in.
const UNAUTHORIZED: u16 = 401;

/// Sender the views use to submit actions; cloneable, survives reconnects.
pub type OutboundTx = UnboundedSender<ClientMessage>;

/// Start the transport: provide an `OutboundTx` and a `CreateTx` into context,
/// then spawn the bootstrap + connect loop. Call once from `App` (wasm only).
pub fn start(store: StoreSignal) {
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let (create_tx, create_rx) = mpsc::unbounded::<CreateGameRequest>();
    // A tab brought back to the front pings at once, rather than waiting on a
    // ping timer the browser may have throttled while the tab was hidden. A
    // dead socket then ends through the silence timeout. The listener lasts as
    // long as the page, so its handle is never used to remove it.
    let ping_tx = tx.clone();
    let _ = window_event_listener_untyped("visibilitychange", move |_| {
        if !document().hidden() {
            let _ = ping_tx.unbounded_send(ClientMessage::Ping);
        }
    });
    provide_context(tx);
    provide_context::<CreateTx>(create_tx);
    spawn_local(run(store, rx, create_rx));
}

async fn run(
    store: StoreSignal,
    mut rx: UnboundedReceiver<ClientMessage>,
    mut create_rx: UnboundedReceiver<CreateGameRequest>,
) {
    // Signed out, the sign-in screen is up and a sign-in reloads the page.
    if !check_sign_in(store).await {
        return;
    }
    let mut game_id: GameId = match bootstrap(store, &mut create_rx).await {
        Some(id) => id,
        None => return, // bootstrap set Failed or SignedOut already
    };

    // Reconnect attempts since the last `Hello`, which sets it back to 0. A
    // socket that opens and fails before `Hello` doesn't reset it, so a
    // half-working server can't hold the client in a 1 s retry loop.
    let mut attempt: u32 = 0;
    loop {
        let outcome = connect_once(&store, &game_id, &mut rx, &mut attempt).await;
        if let ConnectOutcome::Ended(ending) = outcome {
            if !ending.keeps_saved_game() {
                clear_saved_id();
            }
        }
        match outcome {
            // A wire-format skew: the status is already set to VersionMismatch.
            // Stop the loop entirely — retrying just hits the same stale server.
            ConnectOutcome::VersionMismatch => return,
            // The session is gone. Show the sign-in screen and stop, keeping
            // the saved id: a sign-in reloads the page onto the same game.
            ConnectOutcome::Ended(SocketEnding::Unauthenticated) => {
                // If `/auth/me` still signs us in, the server refused the
                // socket a session it accepts over HTTP, which only a server
                // bug does. Nothing is live, so say so rather than leave the
                // status where the socket left it.
                if check_sign_in(store).await {
                    store.update(|s| s.status = ConnStatus::Failed);
                }
                return;
            }
            // This server's build can't rebuild the game, and every attempt
            // would fail the same way. Say so and stop, keeping the saved id:
            // a rollback to an earlier build can still resume it.
            ConnectOutcome::Ended(SocketEnding::Unresumable) => {
                store.update(|s| s.status = ConnStatus::Unresumable);
                return;
            }
            // The server says it has no game with this id (e.g. a DB reset).
            // It was discarded above; create a fresh game.
            ConnectOutcome::Ended(SocketEnding::StaleId) => {
                let Some(req) = await_roster(&store, &mut create_rx).await else {
                    return;
                };
                match create_game(store, req).await {
                    Some(id) => game_id = id,
                    None => return,
                }
            }
            // Couldn't reach the server, or a normal disconnect after a
            // live session — keep the SAME id and just retry. The server
            // may be restarting; recreating here would abandon the game
            // (and wipe the saved id) on every transient outage.
            ConnectOutcome::Unreachable | ConnectOutcome::Ended(SocketEnding::Disconnected) => {}
        }

        store.update(|s| s.status = ConnStatus::Reconnecting);
        TimeoutFuture::new(millis(socket_ending::backoff(attempt))).await;
        attempt = attempt.saturating_add(1);
    }
}

/// Outcome of one socket lifetime — the reconnect loop treats each case
/// differently (only a stale id discards the saved game id).
enum ConnectOutcome {
    /// `WebSocket::open` failed: the server is unreachable (down or
    /// restarting). Keep the id and retry.
    Unreachable,
    /// Opened, then closed, sorted by [`socket_ending::sort`].
    Ended(SocketEnding),
    /// A server frame failed to deserialize — the client and server binaries
    /// disagree on the wire format. Terminal: do not retry (a retry hits the
    /// same stale server). The status is set to `VersionMismatch` before this
    /// is returned.
    VersionMismatch,
}

/// Ask the server who this client is signed in as. Signed in, record the
/// account and return `true`. Signed out, show the sign-in screen with the
/// methods the server listed and return `false`; any other answer sets
/// `Failed` and returns `false` too.
async fn check_sign_in(store: StoreSignal) -> bool {
    let Ok(resp) = Request::get("/auth/me").send().await else {
        store.update(|s| s.status = ConnStatus::Failed);
        return false;
    };
    if resp.ok() {
        if let Ok(account) = resp.json::<SignedIn>().await {
            store.update(|s| s.account = Some(account));
            return true;
        }
    } else if resp.status() == UNAUTHORIZED {
        if let Ok(required) = resp.json::<SignInRequired>().await {
            store.update(|s| {
                s.account = None;
                s.status = ConnStatus::SignedOut(required.methods);
            });
            return false;
        }
    }
    store.update(|s| s.status = ConnStatus::Failed);
    false
}

/// Resolve a game id: reuse a saved one, else await a roster from the picker
/// and create. Returns `None` (and sets `Failed` or `SignedOut`) if
/// creation fails.
async fn bootstrap(
    store: StoreSignal,
    create_rx: &mut UnboundedReceiver<CreateGameRequest>,
) -> Option<GameId> {
    if let Some(id) = saved_id() {
        return Some(id);
    }
    let req = await_roster(&store, create_rx).await?;
    create_game(store, req).await
}

/// Set status to `AwaitingRoster` and block until the picker sends a request.
async fn await_roster(
    store: &StoreSignal,
    create_rx: &mut UnboundedReceiver<CreateGameRequest>,
) -> Option<CreateGameRequest> {
    store.update(|s| s.status = ConnStatus::AwaitingRoster);
    create_rx.next().await
}

async fn create_game(store: StoreSignal, request: CreateGameRequest) -> Option<GameId> {
    let resp = Request::post("/games")
        .json(&request)
        .ok()?
        .send()
        .await
        .ok()?;
    if resp.status() == UNAUTHORIZED {
        // The session ended since boot: show the sign-in screen instead.
        check_sign_in(store).await;
        return None;
    }
    if let Ok(r) = resp.json::<CreateGameResponse>().await {
        save_id(&r.game_id);
        Some(r.game_id)
    } else {
        store.update(|s| s.status = ConnStatus::Failed);
        None
    }
}

/// One socket lifetime: open, pump inbound->reduce and outbound->sink,
/// pinging every [`protocol::PING_INTERVAL`], until close or until the server
/// is silent for [`protocol::CLIENT_SILENCE_TIMEOUT`], and mark Connected
/// when the `Hello` arrives, which also sets `attempt` back to 0. The returned
/// [`ConnectOutcome`] tells the reconnect loop whether the server is
/// unreachable, the frames were skewed, or how the socket ended.
///
/// Until the `Hello`, the board still shows the state from before the drop, so
/// the status stays what it was and the board stays locked. A `Reconnecting`
/// status holds through the attempt, so its banner doesn't flicker away. At the
/// end of the lifetime the outbound channel is drained unsent, so nothing
/// clicked while disconnected goes out on the next socket.
async fn connect_once(
    store: &StoreSignal,
    game_id: &GameId,
    rx: &mut UnboundedReceiver<ClientMessage>,
    attempt: &mut u32,
) -> ConnectOutcome {
    store.update(|s| {
        if s.status != ConnStatus::Reconnecting {
            s.status = ConnStatus::Connecting;
        }
    });
    let Ok(ws) = WebSocket::open(&url::current_ws_url(game_id.as_str())) else {
        return ConnectOutcome::Unreachable;
    };

    let (mut write, read) = ws.split();
    let mut read = read.fuse();
    let mut close_code = None;
    let mut version_mismatch = false;
    let mut ping = IntervalStream::new(millis(protocol::PING_INTERVAL)).fuse();
    let mut silence = silence_deadline();
    let mut silent = false;

    loop {
        select! {
            () = silence => {
                silent = true;
                break;
            }
            _ = ping.next() => send(&mut write, &ClientMessage::Ping).await,
            incoming = read.next() => match incoming {
                Some(Ok(Message::Text(txt))) => {
                    // Any server frame proves the connection alive.
                    silence = silence_deadline();
                    if let Ok(msg) = serde_json::from_str::<ServerMessage>(&txt) {
                        let is_hello = matches!(msg, ServerMessage::Hello { .. });
                        if is_hello {
                            *attempt = 0;
                        }
                        // One update, so the board unlocks with the fresh
                        // state already in it.
                        store.update(|s| {
                            store::reduce(s, msg);
                            if is_hello {
                                s.status = ConnStatus::Connected;
                            }
                        });
                    } else {
                        // A frame we can't parse from a server that IS talking
                        // to us is a wire-format skew, not a transient glitch.
                        store.update(|s| s.status = ConnStatus::VersionMismatch);
                        version_mismatch = true;
                        break;
                    }
                }
                // The close event carries the code the ending is sorted by.
                Some(Err(WebSocketError::ConnectionClose(event))) => {
                    close_code = Some(event.code);
                    break;
                }
                // Binary frames aren't part of the protocol. An error event
                // comes before the close event that ends the stream, so read
                // on to the close for its code.
                Some(Ok(Message::Bytes(_)) | Err(_)) => {}
                None => break,
            },
            outbound = rx.next() => {
                if let Some(message) = outbound {
                    send(&mut write, &message).await;
                }
            }
        }
    }
    // Whatever was queued is for a socket that's gone, so it's dropped unsent.
    while rx.try_recv().is_ok() {}
    if version_mismatch {
        ConnectOutcome::VersionMismatch
    } else if silent {
        // Dropping the socket's halves closes it. Sorted directly rather than
        // by close code: a silent server is a disconnect, before or after
        // `Hello`, and never a reason to forget the saved game.
        ConnectOutcome::Ended(SocketEnding::Disconnected)
    } else {
        ConnectOutcome::Ended(socket_ending::sort(close_code))
    }
}

/// Send `message` on the socket, best-effort: a failed send means the socket
/// is ending, and its close is read on the inbound side.
async fn send(write: &mut SplitSink<WebSocket, Message>, message: &ClientMessage) {
    if let Ok(json) = serde_json::to_string(message) {
        let _ = write.send(Message::Text(json)).await;
    }
}

/// A timer that fires once the server has been silent for
/// [`protocol::CLIENT_SILENCE_TIMEOUT`]. Restarted on every inbound frame.
fn silence_deadline() -> Fuse<TimeoutFuture> {
    TimeoutFuture::new(millis(protocol::CLIENT_SILENCE_TIMEOUT)).fuse()
}

/// `duration` in the whole milliseconds the browser's timers take.
fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).expect("a liveness timing fits a browser timer")
}

fn local_storage() -> Option<Storage> {
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

/// Reload the page, keeping the saved game id — the *rejoin* counterpart to
/// [`start_new_game`].
///
/// Drives the version-mismatch overlay's button: the server-side game persists
/// across a server restart, so the fix for a skew is to pick the same game back
/// up with a freshly-loaded client, not to abandon it.
pub fn reload() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// Sign out, then reload: boot finds no session and shows the sign-in screen.
///
/// The saved game id is kept, so signing back in returns to the same game.
/// The reload happens even if the request fails, and the boot then shows
/// whatever the server says.
pub fn sign_out() {
    spawn_local(async {
        let _ = Request::post("/auth/sign-out").send().await;
        reload();
    });
}

/// Forget the saved game id and reload, so `bootstrap()` re-enters the picker
/// (no saved id → `await_roster`).
///
/// The server-side game persists; this only drops the local pointer. Pairs with
/// the "New game" buttons in the status bar and the unresumable-game overlay.
pub fn start_new_game() {
    clear_saved_id();
    reload();
}
