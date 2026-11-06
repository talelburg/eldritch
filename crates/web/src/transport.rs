//! Browser transport (wasm only): the client's connection to its game.
//!
//! It checks sign-in, reads the game from the page's address, connects its
//! WebSocket, folds inbound frames into the store, forwards outbound actions,
//! and reconnects on close with backoff. A page whose address names no game is
//! the "Your games" page, whose listing it fetches once.

use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future::Fuse;
use futures::stream::SplitSink;
use futures::{select, FutureExt as _, SinkExt as _, StreamExt as _};
use game_core::scenario::ScenarioId;
use gloo_net::http::Request;
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::{Message, WebSocketError};
use gloo_timers::future::{IntervalStream, TimeoutFuture};
use leptos::prelude::*;
use protocol::{
    ClientMessage, CreateGameRequest, CreateGameResponse, GameEntry, GameId, SignInRequired,
    SignedIn,
};
use wasm_bindgen_futures::spawn_local;

use crate::delete_game::DeleteGame;
use crate::socket_ending::{self, NextStep, SocketEnding};
use crate::store::{self, ConnStatus, Received, StoreSignal};
use crate::url;

/// The scenario every new game plays: The Gathering is the only one.
const SCENARIO_ID: &str = "the-gathering";
/// HTTP 401, which the server answers when the request isn't signed in.
const UNAUTHORIZED: u16 = 401;
/// HTTP 204, which the server answers a delete it carried out with.
const NO_CONTENT: u16 = 204;

/// Sender the views use to submit actions; cloneable, survives reconnects.
pub type OutboundTx = UnboundedSender<ClientMessage>;

/// Start the transport: provide an `OutboundTx` and the HTTP [`DeleteGame`]
/// into context, then spawn the connect loop. Call once from `App` (wasm
/// only).
pub fn start(store: StoreSignal) {
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
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
    provide_context(DeleteGame(Callback::new(move |game_id| {
        spawn_local(delete_game(store, game_id));
    })));
    spawn_local(run(store, rx));
}

async fn run(store: StoreSignal, mut rx: UnboundedReceiver<ClientMessage>) {
    // Signed out, the sign-in screen is up and a sign-in reloads the page.
    if !check_sign_in(store).await {
        return;
    }
    let Some(game_id) = url::current_game_id() else {
        // The "Your games" page, listing what [`fetch_games`] returns once;
        // its New game button creates a lobby through [`new_game`] and goes
        // to it. A game that sent this client here says why, in a toast.
        let ended = url::take_current_ended();
        store.update(|s| {
            s.status = ConnStatus::Home;
            s.toast = ended.map(|ended| ended.toast().to_string());
        });
        fetch_games(store).await;
        return;
    };
    store.update(|s| s.game_id = Some(game_id.clone()));

    // Reconnect attempts since the last `Hello` or `Lobby`, which set it back
    // to 0. A socket that opens and fails before either doesn't reset it, so a
    // half-working server can't hold the client in a 1 s retry loop.
    let mut attempt: u32 = 0;
    loop {
        let outcome = connect_once(&store, &game_id, &mut rx, &mut attempt).await;
        // A socket that never opened ended the way a refused connect does.
        let ending = match outcome {
            ConnectOutcome::Ended(ending) => ending,
            ConnectOutcome::Unreachable => SocketEnding::Disconnected,
            // A wire-format skew: the status is already set to VersionMismatch.
            // Stop the loop entirely — retrying just hits the same stale server.
            ConnectOutcome::VersionMismatch => return,
        };
        match ending.next_step() {
            // The session is gone. Show the sign-in screen and stop.
            NextStep::SignIn => {
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
            // would fail the same way. Say so and stop: the link still names
            // the game, and a rollback to an earlier build can resume it. The
            // listing's entry for the game says where it stopped and why.
            NextStep::Unresumable => {
                store.update(|s| s.status = ConnStatus::Unresumable);
                fetch_games(store).await;
                return;
            }
            // The server has no game at this link (a typo, or a DB reset), this
            // account is no longer a member of it, or its creator deleted it. Go to the home page,
            // which offers a new one, in place of it, naming the ending for
            // its toast.
            NextStep::GoHome(ended) => {
                go_to(&url::home_path(ended), Navigation::Replace);
                return;
            }
            // Couldn't reach the server, a disconnect, or a server shutting
            // down for a restart — keep the SAME game and just retry. A
            // restart says so until the next `Hello` or `Lobby`, through
            // however many refused connects it takes.
            NextStep::Reconnect => {}
        }

        store.update(|s| s.status = s.status.retrying_after(ending));
        TimeoutFuture::new(millis(socket_ending::backoff(attempt))).await;
        attempt = attempt.saturating_add(1);
    }
}

/// Outcome of one socket lifetime — the reconnect loop treats each case
/// differently.
enum ConnectOutcome {
    /// `WebSocket::open` failed: the server is unreachable (down or
    /// restarting). Keep the id and retry.
    Unreachable,
    /// Opened, then closed, sorted by [`socket_ending::sort`].
    Ended(SocketEnding),
    /// The `Hello` named another build, or a server frame failed to
    /// deserialize ([`store::receive`]) — the client and server binaries
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

/// `GET /games`, the "Your games" listing, into the store. Signed out since
/// boot, show the sign-in screen instead. Any other failure leaves the
/// listing absent and says why on the console.
async fn fetch_games(store: StoreSignal) {
    let resp = match Request::get("/games").send().await {
        Ok(resp) => resp,
        Err(error) => {
            leptos::logging::warn!("GET /games failed: {error}");
            return;
        }
    };
    if resp.status() == UNAUTHORIZED {
        check_sign_in(store).await;
        return;
    }
    match resp.json::<Vec<GameEntry>>().await {
        Ok(games) => store.update(|s| s.games = Some(games)),
        Err(error) => leptos::logging::warn!("GET /games answered {}: {error}", resp.status()),
    }
}

/// `POST /games` with `request`; return the new game's id. Returns `None`
/// (and sets `Failed` or `SignedOut`) if creation fails.
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
        Some(r.game_id)
    } else {
        store.update(|s| s.status = ConnStatus::Failed);
        None
    }
}

/// `DELETE /games/{id}` of `game_id`, which the creator confirmed. Deleted,
/// its row leaves the "Your games" listing; a lobby or board this page shows
/// leaves when the server closes the socket as deleted. Signed out since boot,
/// show the sign-in screen instead. Any other answer says so in a toast.
async fn delete_game(store: StoreSignal, game_id: GameId) {
    let path = format!("/games/{}", game_id.as_str());
    let status = Request::delete(&path)
        .send()
        .await
        .map(|resp| resp.status());
    match status {
        Ok(NO_CONTENT) => store.update(|s| {
            if let Some(games) = &mut s.games {
                games.retain(|entry| entry.game_id != game_id);
            }
        }),
        Ok(UNAUTHORIZED) => {
            check_sign_in(store).await;
        }
        _ => store.update(|s| s.toast = Some("Couldn't delete the game, try again.".to_owned())),
    }
}

/// One socket lifetime: open, pump inbound->reduce and outbound->sink,
/// pinging every [`protocol::PING_INTERVAL`], until close or until the server
/// is silent for [`protocol::CLIENT_SILENCE_TIMEOUT`]. The `Hello` or `Lobby`
/// the socket opens with sets the status, and `attempt` back to 0. The returned
/// [`ConnectOutcome`] tells the reconnect loop whether the server is
/// unreachable, the frames were skewed, or how the socket ended.
///
/// Until the `Hello`, the board still shows the state from before the drop, so
/// the status stays what it was and the board stays locked. A `Reconnecting`
/// or `Restarting` status holds through the attempt, so its banner doesn't
/// flicker away. At the end of the lifetime the outbound channel is drained
/// unsent, so nothing clicked while disconnected goes out on the next socket.
async fn connect_once(
    store: &StoreSignal,
    game_id: &GameId,
    rx: &mut UnboundedReceiver<ClientMessage>,
    attempt: &mut u32,
) -> ConnectOutcome {
    store.update(|s| {
        if !s.status.is_retrying() {
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
                    // One update, so the board unlocks with the fresh state
                    // already in it.
                    let mut received = Received::Message;
                    store.update(|s| received = store::receive(s, &txt));
                    match received {
                        Received::Hello | Received::Lobby => *attempt = 0,
                        Received::Message => {}
                        // Another build's server, or a frame we can't parse
                        // from a server that IS talking to us: a wire-format
                        // skew, not a transient glitch. Nothing more is sent.
                        Received::VersionMismatch => {
                            version_mismatch = true;
                            break;
                        }
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
        // `Hello`, and never a reason to leave the game.
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

/// How [`go_to`] moves the page.
#[derive(Clone, Copy)]
enum Navigation {
    /// As a link does: the page left stays in the history.
    Assign,
    /// In place of the page left, which the back button then skips.
    Replace,
}

/// Move the page to the same-origin `path`.
fn go_to(path: &str, navigation: Navigation) {
    if let Some(w) = web_sys::window() {
        let location = w.location();
        let _ = match navigation {
            Navigation::Assign => location.assign(path),
            Navigation::Replace => location.replace(path),
        };
    }
}

/// Reload the page, which reopens the game its address names.
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
/// The reload happens even if the request fails, and the boot then shows
/// whatever the server says.
pub fn sign_out() {
    spawn_local(async {
        let _ = Request::post("/auth/sign-out").send().await;
        reload();
    });
}

/// Create a game of The Gathering as a lobby, then go to its address.
///
/// Every New game button calls this: the home page's, the status bar's and
/// the unresumable-game overlay's. A failure is left on `store`'s status.
pub fn new_game(store: StoreSignal) {
    spawn_local(async move {
        let request = CreateGameRequest {
            scenario_id: ScenarioId::new(SCENARIO_ID),
        };
        if let Some(game_id) = create_game(store, request).await {
            go_to(&url::game_path(&game_id), Navigation::Assign);
        }
    });
}
