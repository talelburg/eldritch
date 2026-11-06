//! A game's room: the one place its connections meet. It holds the game in
//! whichever form it is in, a lobby or a live game, behind a mutex so changes
//! to it apply one at a time, and the broadcast channel every connection to it
//! subscribes to.

use std::sync::Arc;

use game_core::engine::Registries;
use game_core::state::investigator::InvestigatorId;
use protocol::live::{Committed, LiveGame};
use protocol::lobby::{Lobby, LobbyRefusal, LobbyView, Member};
use protocol::{AccountId, GameId, Refusal, ServerMessage};
use sqlx::SqlitePool;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::Mutex;

use crate::session::{GameSession, NewGame, ReassignError, SessionError};
use crate::{lobby, store};

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
    /// Deleted by its creator, its rows gone. A connection that got the room
    /// before the delete evicted it is refused, as one to a game that
    /// doesn't exist.
    Deleted,
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
    /// An account opened the started game's link for the first time, and the
    /// members now read like this.
    Members(Arc<Vec<Member>>),
    /// An investigator was reassigned: the [`ServerMessage::Assigned`] every
    /// connection is sent, the same for each account.
    Assigned(Arc<ServerMessage>),
    /// `account` left the lobby or was removed from it, which now reads like
    /// `view`. Every connection of `account`'s closes, and every other one is
    /// sent the lobby.
    Departed {
        /// The account that is no longer a member.
        account: AccountId,
        /// The lobby without it.
        view: Arc<LobbyView>,
    },
    /// The creator deleted the game. Every connection closes.
    Deleted,
}

/// Why [`GameRoom::attach`] attached no connection.
#[derive(Debug)]
pub(crate) enum AttachError {
    /// The game was deleted while the connection was getting its room.
    Deleted,
    /// Storing the membership failed.
    Db(sqlx::Error),
}

/// Why [`GameRoom::delete`] deleted nothing.
#[derive(Debug)]
pub(crate) enum DeleteError {
    /// The account asking isn't the game's creator.
    NotCreator,
    /// The game was already deleted.
    Deleted,
    /// Deleting its rows failed.
    Db(sqlx::Error),
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
    /// the lobby once, with its `joined` flag set. So are a started game's
    /// members, which the joining connection is sent only in its `Hello`.
    ///
    /// # Errors
    ///
    /// [`AttachError::Deleted`] in a deleted room, which stores nothing;
    /// [`AttachError::Db`] if storing the membership failed.
    pub(crate) async fn attach(&self, member: Member) -> Result<Attached, AttachError> {
        let account = member.account;
        let mut game = self.game.lock().await;
        if matches!(*game, RoomGame::Deleted) {
            return Err(AttachError::Deleted);
        }
        let joined = lobby::add_member(&self.db, &self.game_id, account)
            .await
            .map_err(AttachError::Db)?;
        let first = match &mut *game {
            RoomGame::Live(session) => {
                if joined && session.join(member) {
                    // As the lobby's below: nobody else to tell if it fails.
                    let members = session.live().members().to_vec();
                    let _ = self.tx.send(RoomEvent::Members(Arc::new(members)));
                }
                session.live().hello_for(account)
            }
            RoomGame::Lobby(lobby) => {
                if joined && lobby.join(member) {
                    // A broadcast send fails only when no connection is
                    // subscribed, and then nobody else is there to tell.
                    let _ = self.tx.send(RoomEvent::Lobby(Arc::new(lobby.view())));
                }
                lobby_message(account, lobby.view(), joined)
            }
            RoomGame::Deleted => unreachable!("a deleted room is refused above"),
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
        self.apply_to_lobby(None, command).await
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
        self.apply_to_lobby(Some(departing), command).await
    }

    /// Apply `command` to a copy of the room's lobby, store it, dropping
    /// `departing`'s membership if there is one, then replace the lobby and
    /// broadcast it: as [`RoomEvent::Departed`] if an account departed, and
    /// as [`RoomEvent::Lobby`] otherwise.
    async fn apply_to_lobby(
        &self,
        departing: Option<AccountId>,
        command: impl FnOnce(&mut Lobby) -> Result<(), LobbyRefusal>,
    ) -> Result<(), LobbyRefusal> {
        let mut game = self.game.lock().await;
        let RoomGame::Lobby(lobby) = &mut *game else {
            // A deleted room is refused here too. Its connections never see
            // this refusal: the socket forwards the room's events before a
            // reply, and the queued `Deleted` closes it first.
            return Err(LobbyRefusal::Started);
        };
        let mut changed = lobby.clone();
        command(&mut changed)?;
        let view = changed.view();
        self.store_lobby(&view, departing)
            .await
            .map_err(|error| LobbyRefusal::Failed(error.to_string()))?;
        *lobby = changed;
        let view = Arc::new(view);
        let event = match departing {
            Some(account) => RoomEvent::Departed { account, view },
            None => RoomEvent::Lobby(view),
        };
        // Sent under the game's lock, which a connection attaching also holds
        // while it subscribes. A send fails only when no connection is
        // subscribed, and the one that sent this command is.
        let _ = self.tx.send(event);
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
        let session = new_game.into_session(self.db.clone(), lobby.view().members);
        let live = Arc::new(session.live().clone());
        *game = RoomGame::Live(Box::new(session));
        // Sent under the game's lock, as every change is. A send fails only
        // when no connection is subscribed, and the one that sent Start is.
        let _ = self.tx.send(RoomEvent::Started(live));
        Ok(())
    }

    /// **Reassign** `investigator` to the member `to` in the room's live game,
    /// as `by` asks: store the new holder, then broadcast the `Assigned` to
    /// every connection. A refused reassignment, or one that couldn't be
    /// stored, changes nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::Reassign`] with the live game's reason;
    /// [`Refusal::NotStarted`] in a lobby; [`Refusal::Failed`] if it couldn't
    /// be stored.
    pub(crate) async fn reassign(
        &self,
        by: AccountId,
        investigator: InvestigatorId,
        to: AccountId,
    ) -> Result<(), Refusal> {
        let mut game = self.game.lock().await;
        let RoomGame::Live(session) = &mut *game else {
            // A deleted room is refused here too, unseen, as
            // `apply_to_lobby` says.
            return Err(Refusal::NotStarted);
        };
        let assigned =
            session
                .reassign(by, investigator, to)
                .await
                .map_err(|error| match error {
                    ReassignError::Refused(reason) => Refusal::Reassign(reason),
                    ReassignError::Session(error) => Refusal::Failed(error.to_string()),
                })?;
        // Sent under the game's lock, as every change is. A send fails only
        // when no connection is subscribed, and the one that asked is.
        let _ = self.tx.send(RoomEvent::Assigned(Arc::new(assigned)));
        Ok(())
    }

    /// Delete the room's game, lobby or started, as `account` asks: delete
    /// every row stored under it in one transaction, mark the room deleted
    /// and broadcast [`RoomEvent::Deleted`], which closes every connection.
    /// The caller then evicts the room from the registry. Only the creator
    /// may; a refused delete, or one that couldn't be stored, changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`DeleteError::NotCreator`] for anyone else; [`DeleteError::Deleted`]
    /// if it already was; [`DeleteError::Db`] if the rows couldn't be
    /// deleted.
    pub(crate) async fn delete(&self, account: AccountId) -> Result<(), DeleteError> {
        let mut game = self.game.lock().await;
        let creator = match &*game {
            RoomGame::Lobby(lobby) => lobby.creator(),
            RoomGame::Live(session) => session.live().creator(),
            RoomGame::Deleted => return Err(DeleteError::Deleted),
        };
        if account != creator {
            return Err(DeleteError::NotCreator);
        }
        store::delete_game(&self.db, &self.game_id)
            .await
            .map_err(DeleteError::Db)?;
        *game = RoomGame::Deleted;
        // Sent under the game's lock, as every change is. A send fails only
        // when no connection is subscribed, and then there is none to close.
        let _ = self.tx.send(RoomEvent::Deleted);
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
