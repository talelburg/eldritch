//! `GameSession`: the host wrapper pairing the pure engine with the
//! `SQLite` action log.
//!
//! A game is a seed [`GameState`] (the scenario's `setup()` output) plus
//! an ordered sequence of applied actions. The live session keeps the
//! derived state in memory and appends each accepted action to the log;
//! [`GameSession::load`] reconstructs a session by replaying that log
//! over the seed, reproducing state bit-for-bit.

use game_core::scenario::ScenarioId;
use game_core::state::GameState;
use game_core::{Action, EngineOutcome, Event, PlayerAction};
use sqlx::SqlitePool;

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
}

/// A live game: derived state plus its connection to the action log.
pub struct GameSession {
    /// Stable identifier for this game (primary key in `games`).
    pub game_id: String,
    /// Current derived state.
    pub state: GameState,
    /// Outcome of the most recent apply (`Done` for a freshly created
    /// game with no actions yet).
    pub outcome: EngineOutcome,
    /// Next sequence number to assign to a persisted action — equal to
    /// the number of actions already in the log.
    seq: i64,
    /// Connection pool for persistence.
    db: SqlitePool,
}

impl GameSession {
    /// Create a new game from a scenario's `setup()` and persist its
    /// seed state.
    ///
    /// Looks the scenario module up via the installed
    /// [`scenario_registry`](game_core::scenario_registry); returns
    /// [`SessionError::UnknownScenario`] if none is registered.
    pub async fn create(
        db: SqlitePool,
        game_id: impl Into<String>,
        scenario_id: ScenarioId,
    ) -> Result<Self, SessionError> {
        let module = game_core::scenario_registry::current()
            .and_then(|registry| (registry.module_for)(&scenario_id))
            .ok_or_else(|| SessionError::UnknownScenario(scenario_id.clone()))?;

        let state = (module.setup)();
        let seed_state = serde_json::to_string(&state)?;
        let game_id = game_id.into();
        store::insert_game(
            &db,
            &game_id,
            scenario_id.as_str(),
            &seed_state,
            &unix_millis_string(),
        )
        .await?;

        Ok(Self {
            game_id,
            state,
            outcome: EngineOutcome::Done,
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
    pub async fn load(db: SqlitePool, game_id: &str) -> Result<Option<Self>, SessionError> {
        let Some((_scenario_id, seed_state)) = store::load_game(&db, game_id).await? else {
            return Ok(None);
        };

        let mut state: GameState = serde_json::from_str(&seed_state)?;
        let mut outcome = EngineOutcome::Done;
        let mut seq: i64 = 0;
        for action_json in store::load_actions(&db, game_id).await? {
            let action: Action = serde_json::from_str(&action_json)?;
            let result = game_core::apply(state, action);
            state = result.state;
            outcome = result.outcome;
            seq += 1;
        }

        Ok(Some(Self {
            game_id: game_id.to_string(),
            state,
            outcome,
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
