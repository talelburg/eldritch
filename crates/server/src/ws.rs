//! Websocket hub: one broadcast group per game, fanning accepted-action
//! events out to every connection while serializing applies through a
//! per-game [`GameSession`] mutex.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use game_core::EngineOutcome;
use tokio::sync::{broadcast, Mutex};

use crate::session::GameSession;
use crate::wire::{ClientMessage, ServerMessage};
use crate::AppState;

/// Per-game broadcast buffer depth. Generous for the low message rate
/// of a single scenario; a slow client that overruns it gets a
/// `Lagged` and resyncs from subsequent frames rather than blocking
/// the game.
const BROADCAST_CAPACITY: usize = 256;

/// A live game: its session (behind a mutex so applies serialize across
/// connections) and the broadcast channel its connections subscribe to.
pub(crate) struct GameRoom {
    session: Mutex<GameSession>,
    tx: broadcast::Sender<ServerMessage>,
}

/// The server's map of live games, keyed by `game_id`.
pub(crate) type Rooms = Arc<Mutex<HashMap<String, Arc<GameRoom>>>>;

/// Build an empty rooms map for [`AppState`].
pub(crate) fn rooms() -> Rooms {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Get the room for `game_id`, lazily loading it from the action log on
/// a cache miss (e.g. first access, or after a server restart). Returns
/// `None` if no such game exists.
async fn get_or_load_room(state: &AppState, game_id: &str) -> Option<Arc<GameRoom>> {
    let mut rooms = state.rooms.lock().await;
    if let Some(room) = rooms.get(game_id) {
        return Some(room.clone());
    }
    let session = GameSession::load(state.db.clone(), game_id).await.ok()??;
    let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
    let room = Arc::new(GameRoom {
        session: Mutex::new(session),
        tx,
    });
    rooms.insert(game_id.to_string(), room.clone());
    Some(room)
}

/// Axum handler: upgrade a `GET /games/{game_id}/ws` request to a
/// websocket attached to that game's broadcast group.
pub(crate) async fn game_ws(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, game_id))
}

async fn handle_socket(socket: WebSocket, state: AppState, game_id: String) {
    let Some(room) = get_or_load_room(&state, &game_id).await else {
        // No such game — nothing to attach to. Drop the socket.
        return;
    };
    let mut rx = room.tx.subscribe();
    let (mut sink, mut stream) = socket.split();

    // Send the render baseline before streaming events.
    let hello = {
        let session = room.session.lock().await;
        ServerMessage::Hello {
            state: Box::new(session.state.clone()),
            outcome: session.outcome.clone(),
        }
    };
    if send_msg(&mut sink, &hello).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            broadcasted = rx.recv() => match broadcasted {
                Ok(msg) => {
                    if send_msg(&mut sink, &msg).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break };
                if let Some(direct) = handle_client_message(&room, message).await {
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
                    Ok((events, EngineOutcome::Rejected { reason })) => {
                        debug_assert!(events.is_empty());
                        Some(Disposition::Reply(ServerMessage::Rejected {
                            reason: reason.into_owned(),
                        }))
                    }
                    Ok((events, outcome)) => {
                        let _ = room.tx.send(ServerMessage::Applied { events, outcome });
                        None
                    }
                    Err(e) => Some(Disposition::Reply(ServerMessage::Rejected {
                        reason: e.to_string(),
                    })),
                }
            }
            Err(_) => Some(Disposition::Reply(ServerMessage::Rejected {
                reason: "malformed message".to_string(),
            })),
        },
        Message::Close(_) => Some(Disposition::Close),
        // Ping/Pong/Binary are not part of the protocol; ignore them.
        _ => None,
    }
}

async fn send_msg(
    sink: &mut SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(msg).expect("ServerMessage always serializes");
    sink.send(Message::Text(json.into())).await
}
