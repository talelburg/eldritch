//! The room registry: the server's map of live games' rooms, each loaded once
//! per game id through a [`Loader`].

use std::collections::HashMap;
use std::future::Future;
use std::panic;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use protocol::GameId;
use tokio::sync::OnceCell;
use tokio::task;

use crate::session::SessionError;

/// How the registry loads a game's room: an async read of what is stored, then
/// a sync rebuild of the room from it, which the registry runs on a blocking
/// thread.
pub(crate) trait Loader: Send + Sync + 'static {
    /// What [`Loader::read`] fetched for a game.
    type Stored: Send + 'static;
    /// A loaded game's room.
    type Room: Send + Sync + 'static;

    /// Fetch what is stored for `game_id`, or `None` if no such game exists.
    fn read(
        &self,
        game_id: &GameId,
    ) -> impl Future<Output = Result<Option<Self::Stored>, SessionError>> + Send;

    /// Rebuild the room from what [`Loader::read`] fetched. Runs on a blocking
    /// thread, so it may take as long as the replay does.
    #[expect(
        clippy::result_large_err,
        reason = "the error is `GameSession::rebuild`'s, built once per failed \
                  load; the registry shares it behind an `Arc`"
    )]
    fn rebuild(&self, stored: Self::Stored) -> Result<Self::Room, SessionError>;
}

/// Why [`RoomRegistry::get_or_load`] returned no room.
#[derive(Debug, Clone)]
pub(crate) enum LoadFailure {
    /// No game with that id exists.
    NotFound,
    /// The game exists and failed to load, for this reason.
    Failed(Arc<SessionError>),
}

/// What one game's load came to: its room, or why there is none.
type Loaded<R> = Result<Arc<R>, LoadFailure>;

/// One game's entry in the map: a cell its first get fills with the load's
/// result, which every get waiting on it then shares.
struct Entry<R> {
    loaded: OnceCell<Loaded<R>>,
}

/// The live games' rooms, keyed by [`GameId`]. It is the only code that
/// touches the map.
///
/// The map's lock is held only to look an entry up, insert one or remove one,
/// never across an await, so a load holds up only the gets of its own game.
/// What stays in the map after a load:
///
/// - a loaded room;
/// - an [`SessionError::Unresumable`] failure, until the process restarts, so
///   a later get returns it without reading or rebuilding again;
/// - nothing for a missing game or any other failure, so the next get loads
///   again.
pub(crate) struct RoomRegistry<L: Loader> {
    loader: Arc<L>,
    rooms: Mutex<HashMap<GameId, Arc<Entry<L::Room>>>>,
}

impl<L: Loader> RoomRegistry<L> {
    /// An empty registry loading through `loader`.
    pub(crate) fn new(loader: L) -> Self {
        Self {
            loader: Arc::new(loader),
            rooms: Mutex::new(HashMap::new()),
        }
    }

    /// The room for `game_id`, loading it if no get has yet. Gets of one id
    /// that arrive while it loads wait for that load and share its result.
    ///
    /// # Errors
    ///
    /// [`LoadFailure::NotFound`] if no such game exists, or
    /// [`LoadFailure::Failed`] with the error the load failed with.
    pub(crate) async fn get_or_load(&self, game_id: &GameId) -> Loaded<L::Room> {
        let entry = Arc::clone(self.map().entry(game_id.clone()).or_insert_with(|| {
            Arc::new(Entry {
                loaded: OnceCell::new(),
            })
        }));
        entry
            .loaded
            .get_or_init(|| async {
                let loaded = self.load(game_id).await;
                let cached = match &loaded {
                    Ok(_) => true,
                    Err(LoadFailure::NotFound) => false,
                    Err(LoadFailure::Failed(err)) => {
                        matches!(**err, SessionError::Unresumable { .. })
                    }
                };
                if !cached {
                    self.remove(game_id, &entry);
                }
                loaded
            })
            .await
            .clone()
    }

    /// Read `game_id`, then rebuild its room on a blocking thread.
    async fn load(&self, game_id: &GameId) -> Loaded<L::Room> {
        let stored = match self.loader.read(game_id).await {
            Ok(Some(stored)) => stored,
            Ok(None) => return Err(LoadFailure::NotFound),
            Err(err) => return Err(LoadFailure::Failed(Arc::new(err))),
        };
        let loader = Arc::clone(&self.loader);
        task::spawn_blocking(move || {
            loader
                .rebuild(stored)
                .map(Arc::new)
                .map_err(|err| LoadFailure::Failed(Arc::new(err)))
        })
        .await
        .unwrap_or_else(|join| match join.try_into_panic() {
            Ok(payload) => panic::resume_unwind(payload),
            Err(join) => panic!("a room's rebuild was cancelled: {join}"),
        })
    }

    /// Remove `game_id`'s entry if it is still `entry`, and not one a later
    /// get has put in its place.
    fn remove(&self, game_id: &GameId, entry: &Arc<Entry<L::Room>>) {
        let mut rooms = self.map();
        if rooms
            .get(game_id)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
        {
            rooms.remove(game_id);
        }
    }

    /// The map, locked. A panic while it was held left no half-made change,
    /// since every change is one insert or remove, so a poisoned lock is
    /// taken as it stands.
    fn map(&self) -> MutexGuard<'_, HashMap<GameId, Arc<Entry<L::Room>>>> {
        self.rooms.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use tokio::sync::{watch, Notify};

    use super::*;
    use crate::session::UnresumableReason;

    /// What a test game's read finds.
    enum Read {
        Found,
        Missing,
        Fails,
    }

    /// What a test game's rebuild produces.
    enum Rebuild {
        Room,
        Unresumable,
    }

    /// One game as the [`TestLoader`] serves it, with the number of times each
    /// half ran and the gates a test holds them open on.
    struct TestGame {
        read: Read,
        rebuild: Rebuild,
        /// When set, read waits until the sender sends `true`.
        read_gate: Option<watch::Receiver<bool>>,
        /// When set, rebuild blocks until the sender sends.
        rebuild_gate: Option<Mutex<mpsc::Receiver<()>>>,
        /// Notified once rebuild has started.
        rebuild_started: Arc<Notify>,
        reads: AtomicUsize,
        rebuilds: AtomicUsize,
    }

    impl TestGame {
        fn new(read: Read, rebuild: Rebuild) -> Self {
            Self {
                read,
                rebuild,
                read_gate: None,
                rebuild_gate: None,
                rebuild_started: Arc::new(Notify::new()),
                reads: AtomicUsize::new(0),
                rebuilds: AtomicUsize::new(0),
            }
        }

        fn loads() -> Self {
            Self::new(Read::Found, Rebuild::Room)
        }

        /// Hold read open until the returned sender sends `true`.
        fn read_held(mut self) -> (Self, watch::Sender<bool>) {
            let (tx, rx) = watch::channel(false);
            self.read_gate = Some(rx);
            (self, tx)
        }

        /// Hold rebuild open until the returned sender sends.
        fn rebuild_held(mut self) -> (Self, mpsc::Sender<()>) {
            let (tx, rx) = mpsc::channel();
            self.rebuild_gate = Some(Mutex::new(rx));
            (self, tx)
        }
    }

    /// A room a [`TestLoader`] rebuilt, naming its game.
    #[derive(Debug)]
    struct TestRoom(GameId);

    /// A [`Loader`] over a fixed set of [`TestGame`]s. An id it doesn't hold
    /// panics, so every test names the games it gets.
    #[derive(Default)]
    struct TestLoader {
        games: HashMap<GameId, Arc<TestGame>>,
    }

    impl TestLoader {
        /// Serve `game` as `id`; the test keeps its own handle to read the
        /// game's run counts.
        fn with(mut self, id: &str, game: &Arc<TestGame>) -> Self {
            self.games.insert(GameId::new(id), Arc::clone(game));
            self
        }

        fn game(&self, game_id: &GameId) -> &TestGame {
            self.games
                .get(game_id)
                .unwrap_or_else(|| panic!("the test loader holds no {game_id:?}"))
        }
    }

    impl Loader for TestLoader {
        type Stored = GameId;
        type Room = TestRoom;

        async fn read(&self, game_id: &GameId) -> Result<Option<GameId>, SessionError> {
            let game = self.game(game_id);
            game.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &game.read_gate {
                gate.clone()
                    .wait_for(|open| *open)
                    .await
                    .expect("gate sender held");
            }
            match game.read {
                Read::Found => Ok(Some(game_id.clone())),
                Read::Missing => Ok(None),
                Read::Fails => Err(SessionError::Db(sqlx::Error::PoolTimedOut)),
            }
        }

        fn rebuild(&self, game_id: GameId) -> Result<TestRoom, SessionError> {
            let game = self.game(&game_id);
            game.rebuilds.fetch_add(1, Ordering::SeqCst);
            game.rebuild_started.notify_one();
            if let Some(gate) = &game.rebuild_gate {
                gate.lock()
                    .expect("gate lock")
                    .recv()
                    .expect("gate sender held");
            }
            match game.rebuild {
                Rebuild::Room => Ok(TestRoom(game_id)),
                Rebuild::Unresumable => Err(SessionError::Unresumable {
                    game_id,
                    reason: UnresumableReason::Version { found: 0 },
                }),
            }
        }
    }

    /// How many times `game`'s read and rebuild ran.
    fn runs(game: &TestGame) -> (usize, usize) {
        (
            game.reads.load(Ordering::SeqCst),
            game.rebuilds.load(Ordering::SeqCst),
        )
    }

    #[tokio::test]
    async fn a_missing_game_leaves_no_entry() {
        let game = Arc::new(TestGame::new(Read::Missing, Rebuild::Room));
        let registry = RoomRegistry::new(TestLoader::default().with("g", &game));
        let id = GameId::new("g");

        assert!(matches!(
            registry.get_or_load(&id).await,
            Err(LoadFailure::NotFound)
        ));
        assert!(matches!(
            registry.get_or_load(&id).await,
            Err(LoadFailure::NotFound)
        ));

        assert_eq!(runs(&game), (2, 0), "the second get read again");
    }

    #[tokio::test]
    async fn the_same_game_loads_once() {
        let (game, open_read) = TestGame::loads().read_held();
        let game = Arc::new(game);
        let registry = RoomRegistry::new(TestLoader::default().with("g", &game));
        let id = GameId::new("g");

        // `join!` polls its futures in order, so both gets have reached the
        // held read, or the cell waiting on it, before the third opens it.
        let (first, second, ()) = tokio::join!(
            registry.get_or_load(&id),
            registry.get_or_load(&id),
            async {
                open_read.send_replace(true);
            },
        );

        let (first, second) = (first.expect("a room"), second.expect("a room"));
        assert!(Arc::ptr_eq(&first, &second), "both gets got the one room");
        assert_eq!(first.0, id);
        assert_eq!(runs(&game), (1, 1));
    }

    /// Whether `loaded` is the failure an unresumable game loads with.
    fn is_unresumable(loaded: &Loaded<TestRoom>) -> bool {
        matches!(loaded, Err(LoadFailure::Failed(err)) if matches!(**err, SessionError::Unresumable { .. }))
    }

    #[tokio::test]
    async fn an_unresumable_result_is_cached() {
        let game = Arc::new(TestGame::new(Read::Found, Rebuild::Unresumable));
        let registry = RoomRegistry::new(TestLoader::default().with("g", &game));
        let id = GameId::new("g");

        assert!(is_unresumable(&registry.get_or_load(&id).await));
        assert!(is_unresumable(&registry.get_or_load(&id).await));

        assert_eq!(
            runs(&game),
            (1, 1),
            "the second get neither read nor rebuilt"
        );
    }

    #[tokio::test]
    async fn a_database_error_is_not_cached() {
        let game = Arc::new(TestGame::new(Read::Fails, Rebuild::Room));
        let registry = RoomRegistry::new(TestLoader::default().with("g", &game));
        let id = GameId::new("g");

        for _ in 0..2 {
            assert!(matches!(
                registry.get_or_load(&id).await,
                Err(LoadFailure::Failed(err)) if matches!(*err, SessionError::Db(_))
            ));
        }

        assert_eq!(runs(&game), (2, 0), "the second get read again");
    }

    #[tokio::test]
    async fn different_games_dont_wait_on_each_other() {
        let (a, release_a) = TestGame::loads().rebuild_held();
        let (a, b) = (Arc::new(a), Arc::new(TestGame::loads()));
        let registry = RoomRegistry::new(TestLoader::default().with("a", &a).with("b", &b));

        let (a_id, b_id) = (GameId::new("a"), GameId::new("b"));

        let (a_room, ()) = tokio::join!(registry.get_or_load(&a_id), async {
            let b_room = registry.get_or_load(&b_id).await;
            assert_eq!(b_room.expect("b's room").0, b_id);
            release_a.send(()).expect("a's rebuild is still held open");
        });

        assert_eq!(a_room.expect("a's room").0, a_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_rebuild_doesnt_block_the_runtime() {
        let (game, release) = TestGame::loads().rebuild_held();
        let game = Arc::new(game);
        let started = Arc::clone(&game.rebuild_started);
        let registry = Arc::new(RoomRegistry::new(TestLoader::default().with("g", &game)));

        let get = tokio::spawn({
            let registry = Arc::clone(&registry);
            async move { registry.get_or_load(&GameId::new("g")).await }
        });
        started.notified().await;
        // The runtime's one worker runs this task while the rebuild is held.
        tokio::spawn(async {})
            .await
            .expect("the unrelated task ran");
        release.send(()).expect("the rebuild is waiting");

        let room = get.await.expect("the get ran").expect("a room");
        assert_eq!(room.0, GameId::new("g"));
    }
}
