//! `GameSession`: the host wrapper pairing the pure engine with the
//! `SQLite` action log.
//!
//! A game is a seed [`GameState`] (the scenario's `setup()` output after
//! seating runs) plus an ordered sequence of applied actions. The live
//! session keeps the derived state in memory and appends each accepted
//! action to the log; [`GameSession::load`] reconstructs a session by
//! replaying that log over the seed, reproducing state bit-for-bit.

use game_core::action::RosterEntry;
use game_core::rng::RngState;
use game_core::scenario::ScenarioId;
use game_core::state::GameState;
use game_core::{Action, EngineOutcome, Event, PlayerAction};
use sqlx::SqlitePool;

use crate::id::GameId;
use crate::store;

/// Errors from [`GameSession`] persistence operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// A database / persistence failure.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    /// (De)serializing state or action JSON failed.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// No scenario module is registered for the requested id.
    #[error("unknown scenario: {}", .0.as_str())]
    UnknownScenario(ScenarioId),
    /// Seating the roster at creation was rejected by the engine (empty
    /// roster, unknown/non-investigator code, or an already-started seed).
    #[error("seating rejected: {0}")]
    Seating(String),
}

/// A live game: derived state plus its connection to the action log.
pub struct GameSession {
    /// Stable identifier for this game (primary key in `games`).
    pub game_id: GameId,
    /// Current derived state.
    pub state: GameState,
    /// Outcome of the most recent apply, or — for a freshly created game
    /// with no actions yet — the seed outcome (the setup mulligan prompt,
    /// i.e. `AwaitingInput`).
    pub outcome: EngineOutcome,
    /// Events emitted during `seat_and_open` at creation time, surfaced to
    /// newly-connecting clients via `ServerMessage::Hello` so the event log
    /// can show opening draws, shuffles, and the weakness set-aside.
    /// Empty for a session reloaded from the DB — setup already ran and its
    /// events are not recoverable from the persisted seed state.
    pub setup_events: Vec<Event>,
    /// Next sequence number to assign to a persisted action — equal to
    /// the number of actions already in the log.
    seq: i64,
    /// Connection pool for persistence.
    db: SqlitePool,
}

impl GameSession {
    /// Create a new game: seats the roster into the scenario's `setup()`
    /// state, persists the seated seed and its outcome, and returns a live
    /// session at the mulligan prompt.
    ///
    /// Looks the scenario module up via the installed
    /// [`scenario_registry`](game_core::scenario_registry).
    ///
    /// # Errors
    ///
    /// - [`SessionError::UnknownScenario`] if no module is registered for
    ///   `scenario_id`.
    /// - [`SessionError::Seating`] if the engine rejects the roster (empty
    ///   roster, unknown investigator code, etc.).
    /// - [`SessionError::Serde`] / [`SessionError::Db`] if persisting the seed
    ///   state fails.
    pub async fn create(
        db: SqlitePool,
        game_id: impl Into<GameId>,
        scenario_id: ScenarioId,
        roster: Vec<RosterEntry>,
    ) -> Result<Self, SessionError> {
        let module = game_core::scenario_registry::current()
            .and_then(|registry| (registry.module_for)(&scenario_id))
            .ok_or_else(|| SessionError::UnknownScenario(scenario_id.clone()))?;

        let mut setup = (module.setup)();
        // Seed the setup shuffle with fresh host entropy (#467). The scenario's
        // setup() builds with a fixed builder seed (game-core is no-I/O, so it
        // can't source randomness itself); without this override every game would
        // share one shuffle/draw order. seat_and_open runs the shuffle below, and
        // the resulting post-shuffle RngState is frozen into seed_state, so replay
        // stays deterministic from the seed alone (the seed needs no separate
        // recording).
        setup.rng = RngState::new(crate::id::random_seed());
        // Human play surfaces skill-test results with a Confirm-to-dismiss step
        // (#478); the engine gates that pause on this flag (default off for tests
        // and non-interactive consumers). The flag persists through seating.
        setup.interactive_acknowledge = true;
        let result = game_core::seat_and_open(setup, &roster);
        let outcome = match result.outcome {
            EngineOutcome::Rejected { reason } => {
                return Err(SessionError::Seating(reason.to_string()))
            }
            other => other,
        };
        let setup_events = result.events;
        let state = result.state;
        let seed_state = serde_json::to_string(&state)?;
        let seed_outcome = serde_json::to_string(&outcome)?;
        let game_id = game_id.into();
        store::insert_game(
            &db,
            &game_id,
            &scenario_id,
            &seed_state,
            &seed_outcome,
            &unix_millis_string(),
        )
        .await?;

        Ok(Self {
            game_id,
            state,
            outcome,
            setup_events,
            seq: 0,
            db,
        })
    }

    /// Apply a player action: validate via the engine, persist it on
    /// acceptance, and advance the in-memory state.
    ///
    /// A rejected action persists nothing and leaves the state
    /// unchanged (the engine guarantees this); the returned
    /// [`EngineOutcome`] carries the rejection reason.
    ///
    /// # Errors
    ///
    /// [`SessionError::Serde`] if the action fails to serialize, or
    /// [`SessionError::Db`] if persisting the accepted action fails. An engine
    /// *rejection* is not an error — it is returned in the `EngineOutcome`.
    pub async fn apply(
        &mut self,
        action: PlayerAction,
    ) -> Result<(Vec<Event>, EngineOutcome), SessionError> {
        let logged = Action::Player(action);
        let result = game_core::apply(self.state.clone(), logged.clone());

        if !matches!(result.outcome, EngineOutcome::Rejected { .. }) {
            let action_json = serde_json::to_string(&logged)?;
            store::insert_action(&self.db, &self.game_id, self.seq, &action_json).await?;
            self.seq += 1;
            self.state = result.state;
            self.outcome = result.outcome.clone();
        }

        Ok((result.events, result.outcome))
    }

    /// Reconstruct a session by replaying its action log over the seed
    /// state. Returns `None` if no game with `game_id` exists.
    ///
    /// `outcome` is initialised from the persisted seed outcome (not
    /// `Done`), so a freshly-created game with an empty log — whose seed
    /// is already `AwaitingInput` (the setup mulligan) — loads correctly.
    /// Each replayed action overwrites `outcome` exactly as today.
    ///
    /// # Errors
    ///
    /// [`SessionError::Db`] if a query fails, or [`SessionError::Serde`] if the
    /// persisted seed state or a logged action fails to deserialize.
    pub async fn load(db: SqlitePool, game_id: &GameId) -> Result<Option<Self>, SessionError> {
        let Some((_scenario_id, seed_state, seed_outcome)) = store::load_game(&db, game_id).await?
        else {
            return Ok(None);
        };

        let mut state: GameState = serde_json::from_str(&seed_state)?;
        let mut outcome: EngineOutcome = serde_json::from_str(&seed_outcome)?;
        let mut seq: i64 = 0;
        for action_json in store::load_actions(&db, game_id).await? {
            let action: Action = serde_json::from_str(&action_json)?;
            let result = game_core::apply(state, action);
            state = result.state;
            outcome = result.outcome;
            seq += 1;
        }

        Ok(Some(Self {
            game_id: game_id.clone(),
            state,
            outcome,
            setup_events: Vec::new(),
            seq,
            db,
        }))
    }
}

/// Milliseconds since the Unix epoch, as a string, for the `created_at`
/// column. Diagnostic only — never replay-load-bearing.
fn unix_millis_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(|_| "0".to_string(), |d| d.as_millis().to_string())
}
