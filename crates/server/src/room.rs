//! A game's room: the one place its connections meet. It holds the game in
//! whichever form it is in, a lobby or a live game, behind a mutex so changes
//! to it apply one at a time, and the broadcast channel every connection to it
//! subscribes to.

use std::sync::Arc;

use protocol::live::Committed;
use protocol::lobby::{Lobby, LobbyView, Member};
use protocol::{AccountId, GameId, ServerMessage};
use sqlx::SqlitePool;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::Mutex;

use crate::lobby;
use crate::session::GameSession;

/// Per-game broadcast buffer depth. Generous for the low message rate
/// of a single scenario; a slow client that overruns it gets a
/// `Lagged` and resyncs from subsequent frames rather than blocking
/// the game.
const BROADCAST_CAPACITY: usize = 256;

/// A loaded game's room.
pub(crate) struct GameRoom {
    game_id: GameId,
    /// The game, behind a mutex so changes serialize across connections.
    pub(crate) game: Mutex<RoomGame>,
    /// What the room broadcasts. Each event is sent once, shared: every
    /// connection builds the message for its own account from it.
    pub(crate) tx: Sender<RoomEvent>,
    /// Where memberships are stored.
    db: SqlitePool,
}

/// The game a room holds.
pub(crate) enum RoomGame {
    /// Still in its lobby.
    Lobby(Lobby),
    /// Started. Boxed: a live game dwarfs a lobby.
    Live(Box<GameSession>),
}

/// One change a room broadcasts to its connections.
#[derive(Debug, Clone)]
pub(crate) enum RoomEvent {
    /// The lobby changed, and now reads like this.
    Lobby(Arc<LobbyView>),
    /// The live game committed an answer.
    Applied(Arc<Committed>),
}

/// What a connection gets on attaching to a room: its subscription and the
/// first message it is sent.
pub(crate) struct Attached {
    /// Every event broadcast after the first message was built.
    pub(crate) rx: Receiver<RoomEvent>,
    /// The connection's first message: a `Hello` or a `Lobby`.
    pub(crate) first: ServerMessage,
}

impl GameRoom {
    /// A room for `game` under `game_id`, storing memberships in `db`.
    pub(crate) fn new(game_id: GameId, game: RoomGame, db: SqlitePool) -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            game_id,
            game: Mutex::new(game),
            tx,
            db,
        }
    }

    /// Attach a connection signed in as `member`: make it a member of the
    /// game if it isn't one, subscribe it, and build its first message.
    ///
    /// All of it happens under the game's lock, which every change also
    /// broadcasts under, so every event the subscription receives is one the
    /// first message doesn't already include. A lobby that this attach joins
    /// is broadcast before the subscription, so the joining connection is sent
    /// the lobby once, with its `joined` flag set.
    ///
    /// # Errors
    ///
    /// Storing the membership failed.
    pub(crate) async fn attach(&self, member: Member) -> Result<Attached, sqlx::Error> {
        let account = member.account;
        let mut game = self.game.lock().await;
        let joined = lobby::add_member(&self.db, &self.game_id, account).await?;
        let first = match &mut *game {
            RoomGame::Live(session) => session.live().hello_for(account),
            RoomGame::Lobby(lobby) => {
                if joined && lobby.join(member) {
                    // A broadcast send fails only when no connection is
                    // subscribed, and then nobody else is there to tell.
                    let _ = self.tx.send(RoomEvent::Lobby(Arc::new(lobby.view())));
                }
                lobby_message(account, lobby.view(), joined)
            }
        };
        Ok(Attached {
            rx: self.tx.subscribe(),
            first,
        })
    }
}

/// The `Lobby` a connection signed in as `you` is sent of `view`.
pub(crate) fn lobby_message(you: AccountId, view: LobbyView, joined: bool) -> ServerMessage {
    ServerMessage::Lobby {
        build: protocol::build_id().to_string(),
        you,
        view,
        joined,
    }
}
