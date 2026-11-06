//! A game's room: the one place its connections meet. It holds the game in
//! whichever form it is in, a lobby or a live game, behind a mutex so changes
//! to it apply one at a time, and the broadcast channel every connection to it
//! subscribes to.

use std::sync::Arc;

use game_core::engine::Registries;
use protocol::live::{Committed, LiveGame};
use protocol::lobby::{Lobby, LobbyRefusal, LobbyView, Member};
use protocol::{AccountId, GameId, ServerMessage};
use sqlx::SqlitePool;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::Mutex;

use crate::lobby;
use crate::session::{GameSession, NewGame, SessionError};

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
    /// Where memberships, the lobby and the started game are stored.
    db: SqlitePool,
    /// What Start seats the game against.
    registries: Registries,
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
    /// The lobby started, and is now this live game: each connection is sent
    /// the `Hello` for its account built from it, so an answer committed
    /// after Start reaches it once, as an `Applied`.
    Started(Arc<LiveGame>),
    /// `account` left the lobby or was removed from it, which now reads like
    /// `view`. Every connection of `account`'s closes, and every other one is
    /// sent the lobby.
    Departed {
        /// The account that is no longer a member.
        account: AccountId,
        /// The lobby without it.
        view: Arc<LobbyView>,
    },
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
    /// A room for `game` under `game_id`, storing in `db`, which starts a
    /// lobby against `registries`.
    pub(crate) fn new(
        game_id: GameId,
        game: RoomGame,
        db: SqlitePool,
        registries: Registries,
    ) -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            game_id,
            game: Mutex::new(game),
            tx,
            db,
            registries,
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

    /// Apply one lobby command, `command`, to the room's lobby: store what it
    /// changed and broadcast the lobby to every connection. One command at a
    /// time per room, so of two racing Takes the second is refused.
    ///
    /// `command` runs on a copy, which replaces the lobby only once it is
    /// stored: a refused command, or one that couldn't be saved, changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// The command's own refusal; [`LobbyRefusal::Started`] in a live room;
    /// [`LobbyRefusal::Failed`] if it couldn't be stored.
    pub(crate) async fn change_lobby(
        &self,
        command: impl FnOnce(&mut Lobby) -> Result<(), LobbyRefusal>,
    ) -> Result<(), LobbyRefusal> {
        let mut game = self.game.lock().await;
        let RoomGame::Lobby(lobby) = &mut *game else {
            return Err(LobbyRefusal::Started);
        };
        let mut changed = lobby.clone();
        command(&mut changed)?;
        let view = changed.view();
        self.store_lobby(&view, None)
            .await
            .map_err(|error| LobbyRefusal::Failed(error.to_string()))?;
        *lobby = changed;
        // Sent under the game's lock, which a connection attaching also holds
        // while it subscribes. A send fails only when no connection is
        // subscribed, and the one that sent this command is.
        let _ = self.tx.send(RoomEvent::Lobby(Arc::new(view)));
        Ok(())
    }

    /// Apply `command`, which makes `departing` no longer a member of the
    /// room's lobby, as [`change_lobby`](Self::change_lobby) applies a
    /// command: on a copy, stored before it replaces the lobby. The
    /// membership's row goes in the transaction that stores the picks, and
    /// the room broadcasts [`RoomEvent::Departed`], which closes every
    /// connection of `departing`'s and sends everyone else the new lobby.
    ///
    /// # Errors
    ///
    /// The command's own refusal; [`LobbyRefusal::Started`] in a live room;
    /// [`LobbyRefusal::Failed`] if it couldn't be stored.
    pub(crate) async fn depart(
        &self,
        departing: AccountId,
        command: impl FnOnce(&mut Lobby) -> Result<(), LobbyRefusal>,
    ) -> Result<(), LobbyRefusal> {
        let mut game = self.game.lock().await;
        let RoomGame::Lobby(lobby) = &mut *game else {
            return Err(LobbyRefusal::Started);
        };
        let mut changed = lobby.clone();
        command(&mut changed)?;
        let view = changed.view();
        self.store_lobby(&view, Some(departing))
            .await
            .map_err(|error| LobbyRefusal::Failed(error.to_string()))?;
        *lobby = changed;
        // Sent under the game's lock, as `change_lobby`'s is.
        let _ = self.tx.send(RoomEvent::Departed {
            account: departing,
            view: Arc::new(view),
        });
        Ok(())
    }

    /// Store `view`'s picks and lead, and drop `departing`'s membership if
    /// there is one, in one transaction.
    async fn store_lobby(
        &self,
        view: &LobbyView,
        departing: Option<AccountId>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;
        lobby::save_picks(&mut tx, &self.game_id, view).await?;
        if let Some(account) = departing {
            lobby::remove_member(&mut tx, &self.game_id, account).await?;
        }
        tx.commit().await
    }

    /// Start the room's lobby as `account` asks: seat its picks, store the
    /// game under the lobby's id and delete the lobby, in one transaction,
    /// then switch the room to the live game in place and broadcast it, so
    /// every connection is sent its `Hello` (ADR 0024).
    ///
    /// A Start refused, by the lobby or by the engine, or that couldn't be
    /// stored changes nothing: the lobby stays as it was, in the room and in
    /// the database.
    ///
    /// # Errors
    ///
    /// [`Lobby::start`]'s refusal; [`LobbyRefusal::Started`] in a live room;
    /// [`LobbyRefusal::Seating`] if the engine refuses the seating; and
    /// [`LobbyRefusal::Failed`] if it couldn't be stored.
    pub(crate) async fn start(&self, account: AccountId) -> Result<(), LobbyRefusal> {
        let mut game = self.game.lock().await;
        let RoomGame::Lobby(lobby) = &*game else {
            return Err(LobbyRefusal::Started);
        };
        let start = lobby.start(account)?;
        let new_game = GameSession::set_up(self.registries, self.game_id.clone(), account, start)
            .map_err(|error| match error {
            SessionError::Seating(reason) => LobbyRefusal::Seating(reason),
            error => LobbyRefusal::Failed(error.to_string()),
        })?;
        self.store_start(&new_game)
            .await
            .map_err(|error| LobbyRefusal::Failed(error.to_string()))?;
        let session = new_game.into_session(self.db.clone());
        let live = Arc::new(session.live().clone());
        *game = RoomGame::Live(Box::new(session));
        // Sent under the game's lock, as every change is. A send fails only
        // when no connection is subscribed, and the one that sent Start is.
        let _ = self.tx.send(RoomEvent::Started(live));
        Ok(())
    }

    /// Store `new_game` and delete the lobby it starts, in one transaction.
    async fn store_start(&self, new_game: &NewGame) -> Result<(), SessionError> {
        let mut tx = self.db.begin().await?;
        new_game.store(&mut tx).await?;
        lobby::delete(&mut tx, &self.game_id).await?;
        tx.commit().await?;
        Ok(())
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
