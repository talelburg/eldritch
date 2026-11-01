//! Websocket hub: one broadcast group per game, fanning accepted-action
//! events out to every connection while serializing applies through a
//! per-game [`GameSession`] mutex.

use std::sync::Arc;

use axum::extract::ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::{IntoResponse as _, Response};
use axum::Error;
use axum_extra::extract::cookie::CookieJar;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt as _, StreamExt as _};
use game_core::engine::Registries;
use protocol::{
    ClientMessage, GameId, ServerMessage, CLOSE_GAME_NOT_FOUND, CLOSE_UNAUTHENTICATED,
    CLOSE_UNRESUMABLE,
};
use sqlx::SqlitePool;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Sender;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{self, Instant};

use crate::rooms::{LoadFailure, Loader, RoomRegistry};
use crate::session::{GameSession, SessionError, StoredGame};
use crate::{auth, AppState};

/// Per-game broadcast buffer depth. Generous for the low message rate
/// of a single scenario; a slow client that overruns it gets a
/// `Lagged` and resyncs from subsequent frames rather than blocking
/// the game.
const BROADCAST_CAPACITY: usize = 256;

/// A loaded game's room: its session (behind a mutex so applies serialize across
/// connections) and the broadcast channel its connections subscribe to.
pub(crate) struct GameRoom {
    session: Mutex<GameSession>,
    tx: Sender<ServerMessage>,
}

/// The production [`Loader`]: a game's room is loaded from its stored game,
/// read from `db` and rebuilt against `registries`.
pub(crate) struct StoredGames {
    db: SqlitePool,
    registries: Registries,
}

impl Loader for StoredGames {
    type Stored = StoredGame;
    type Room = GameRoom;

    async fn read(&self, game_id: &GameId) -> Result<Option<StoredGame>, SessionError> {
        GameSession::read(&self.db, game_id).await
    }

    fn rebuild(&self, stored: StoredGame) -> Result<GameRoom, SessionError> {
        let session = GameSession::rebuild(self.db.clone(), self.registries, stored)?;
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(GameRoom {
            session: Mutex::new(session),
            tx,
        })
    }

    /// An unresumable game fails the same way on every load until the build
    /// changes, which restarts the process. Every other failure is retried
    /// by the next get.
    fn is_permanent(&self, error: &SessionError) -> bool {
        matches!(error, SessionError::Unresumable { .. })
    }
}

/// The server's live games' rooms, loaded from their stored games.
pub(crate) type Rooms = Arc<RoomRegistry<StoredGames>>;

/// Build an empty room registry for [`AppState`], loading from `db` against
/// `registries`.
pub(crate) fn rooms(db: SqlitePool, registries: Registries) -> Rooms {
    Arc::new(RoomRegistry::new(StoredGames { db, registries }))
}

/// Axum handler: upgrade a `GET /ws/{game_id}` request to a
/// websocket attached to that game's broadcast group.
///
/// The session is resolved before the game. Without one, the socket is
/// upgraded and then closed with [`CLOSE_UNAUTHENTICATED`] before any
/// message, rather than refused: a refused upgrade reaches script only as
/// close code 1006, which reads the same as a network drop.
///
/// An unknown game id is closed with [`CLOSE_GAME_NOT_FOUND`] and an
/// unresumable game with [`CLOSE_UNRESUMABLE`], the same way.
pub(crate) async fn game_ws(
    State(state): State<AppState>,
    Path(game_id): Path<GameId>,
    jar: CookieJar,
    ws: WebSocketUpgrade,
) -> Response {
    match auth::signed_in(&state.db, &jar).await {
        Ok(Some(_)) => ws.on_upgrade(move |socket| handle_socket(socket, state, game_id)),
        Ok(None) => {
            ws.on_upgrade(|socket| close_with(socket, CLOSE_UNAUTHENTICATED, "not signed in"))
        }
        Err(error) => auth::internal_error("session lookup", &error).into_response(),
    }
}

/// Close `socket` with `code` before any message, then read it until the
/// client's closing frame, so the close handshake completes and the client
/// sees the code.
async fn close_with(mut socket: WebSocket, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    if socket.send(Message::Close(Some(frame))).await.is_err() {
        return;
    }
    while let Some(Ok(_)) = socket.recv().await {}
}

async fn handle_socket(socket: WebSocket, state: AppState, game_id: GameId) {
    // The connection lives as long as this socket does, however the socket
    // ends, so the room's idle grace starts only once its last socket has gone.
    let connection = match state.rooms.get_or_load(&game_id).await {
        Ok(connection) => connection,
        // No such game: the one ending that tells the client its id is stale.
        Err(LoadFailure::NotFound) => {
            close_with(socket, CLOSE_GAME_NOT_FOUND, "no such game").await;
            return;
        }
        // A game that can't be rebuilt is never served half-built, nor passed
        // off as absent. An unresumable one says so: it fails the same way on
        // every attempt, so the client stops retrying.
        Err(LoadFailure::Failed(err)) if matches!(*err, SessionError::Unresumable { .. }) => {
            tracing::error!(
                game_id = game_id.as_str(),
                error = %err,
                "refusing to attach: this game is unresumable",
            );
            close_with(socket, CLOSE_UNRESUMABLE, "unresumable game").await;
            return;
        }
        // Any other failure is retried by the next get, so the socket is
        // dropped bare and the client reconnects.
        Err(LoadFailure::Failed(err)) => {
            tracing::error!(
                game_id = game_id.as_str(),
                error = %err,
                "refusing to attach: this game failed to load",
            );
            return;
        }
    };
    let room = connection.room();
    let mut rx = room.tx.subscribe();
    let (mut sink, mut stream) = socket.split();

    // Send the render baseline before streaming events.
    let hello = room.session.lock().await.live().hello();
    if send_msg(&mut sink, &hello).await.is_err() {
        return;
    }

    // Every inbound frame pushes the deadline back. A select arm runs to
    // completion before the loop polls again, so it can't pass mid-apply.
    let silence = time::sleep(state.silence_timeout);
    tokio::pin!(silence);

    loop {
        tokio::select! {
            () = &mut silence => {
                // Best-effort: a client this silent is likely gone, so its
                // close reply isn't awaited.
                let frame = CloseFrame {
                    code: close_code::AWAY,
                    reason: "no frame within the silence timeout".into(),
                };
                let _ = sink.send(Message::Close(Some(frame))).await;
                break;
            }
            broadcasted = rx.recv() => match broadcasted {
                Ok(msg) => {
                    if send_msg(&mut sink, &msg).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break };
                silence.as_mut().reset(Instant::now() + state.silence_timeout);
                if let Some(direct) = handle_client_message(room, message).await {
                    let close = matches!(direct, Disposition::Close);
                    if let Disposition::Reply(msg) = direct {
                        if send_msg(&mut sink, &msg).await.is_err() {
                            break;
                        }
                    }
                    if close {
                        break;
                    }
                }
            }
        }
    }
}

/// What to do with `sink` after handling one inbound frame.
enum Disposition {
    /// Send this message directly to the submitting client only.
    Reply(ServerMessage),
    /// Close the connection.
    Close,
}

/// Apply one inbound frame. Accepted actions broadcast `Applied` to the
/// whole room (returning `None` here — the sender receives it via its
/// own subscription); rejections and malformed frames reply to the
/// sender only.
async fn handle_client_message(room: &GameRoom, message: Message) -> Option<Disposition> {
    match message {
        Message::Text(text) => match serde_json::from_str::<ClientMessage>(text.as_str()) {
            Ok(ClientMessage::Submit { action }) => {
                let mut session = room.session.lock().await;
                match session.apply(action).await {
                    Ok(applied) => {
                        // A broadcast send fails only when no receiver exists.
                        // This task subscribed before its loop and holds that
                        // receiver throughout, so the send cannot fail here, and
                        // `apply` has already persisted the action either way.
                        let _ = room.tx.send(applied);
                        None
                    }
                    Err(e) => Some(Disposition::Reply(ServerMessage::Rejected {
                        reason: e.to_string(),
                    })),
                }
            }
            Ok(ClientMessage::Ping) => Some(Disposition::Reply(ServerMessage::Pong)),
            Err(_) => Some(Disposition::Reply(ServerMessage::Rejected {
                reason: "malformed message".to_string(),
            })),
        },
        Message::Close(_) => Some(Disposition::Close),
        // WebSocket-level ping/pong and binary frames are not part of the
        // protocol; ignore them. They still reset the silence deadline.
        _ => None,
    }
}

async fn send_msg(
    sink: &mut SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
) -> Result<(), Error> {
    let json = serde_json::to_string(msg).expect("ServerMessage always serializes");
    sink.send(Message::Text(json.into())).await
}
