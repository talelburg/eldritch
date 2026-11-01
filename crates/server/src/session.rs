//! `GameSession`: the host wrapper pairing a [`LiveGame`] with the `SQLite`
//! action log.
//!
//! A **stored game** is its creation inputs (scenario, roster, lead, RNG seed,
//! the `interactive_acknowledge` flag) plus an ordered log of its answers
//! (ADR 0022). No engine state is stored. The session keeps the live game in
//! memory and appends each answer it accepts to the log as a [`LoggedAnswer`]
//! with the RNG checkpoint after it, before committing it; [`GameSession::load`]
//! re-runs setup from the inputs, checks it against the stored
//! [`SetupFingerprint`], and replays the log, checking each answer, reproducing
//! state bit-for-bit. A stored game that fails a check is an **unresumable
//! game**: the load fails with [`SessionError::Unresumable`] and its rows are
//! kept.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{Action, PlayerAction, RosterEntry};
use game_core::engine::{self, ApplyResult, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::rng::RngState;
use game_core::scenario::ScenarioId;
use game_core::state::game_state::GameState;
use protocol::live::{LiveGame, Refusal};
use protocol::logged_answer::{self, LoggedAnswer, ReplayFailure};
use protocol::{GameId, ServerMessage};
use sqlx::SqlitePool;

use crate::fingerprint::{FingerprintMismatch, SetupFingerprint};
use crate::id;
use crate::store::{self, ActionRow, GameRow};

/// The version of the stored shape: the creation inputs and a log entry. Bumped
/// whenever either changes; a game stored under any other version is
/// unresumable ([`UnresumableReason::Version`]).
pub const FORMAT_VERSION: i64 = 2;

/// Errors from [`GameSession`] persistence operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// A database / persistence failure.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    /// (De)serializing a stored column or an action's JSON failed.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// No scenario module is registered for the requested id.
    #[error("unknown scenario: {}", .0.as_str())]
    UnknownScenario(ScenarioId),
    /// Seating the roster at creation was rejected by the engine (empty
    /// roster, unknown/non-investigator code, an unimplemented card in the
    /// roster, more random basic weaknesses than the weakness pool holds, or
    /// an already-started seed).
    #[error("seating rejected: {0}")]
    Seating(String),
    /// A stored game exists but this build can't rebuild it as it was played.
    ///
    /// Failing is deliberate: a partially or differently reconstructed session
    /// is a silently wrong board. The game's rows are left as they are, so
    /// rolling the deploy back recovers it.
    #[error("game {} is unresumable: {reason}", .game_id.as_str())]
    Unresumable {
        /// The game that failed to load.
        game_id: GameId,
        /// Why it can't be resumed.
        reason: UnresumableReason,
    },
}

/// Why [`GameSession::apply`] didn't apply an action. Its `Display` is the
/// reason the submitting client is sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApplyError {
    /// The live game refused it. Nothing was stored.
    #[error(transparent)]
    Refused(#[from] Refusal),
    /// The live game accepted it, but storing it failed. Nothing was
    /// committed.
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// Why a stored game is unresumable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UnresumableReason {
    /// The game was stored in a format version this build doesn't load.
    #[error(
        "stored in format version {found}, and this build loads only version {FORMAT_VERSION}"
    )]
    Version {
        /// The version the game was stored in.
        found: i64,
    },
    /// Setup no longer runs from the stored inputs: the scenario is no longer
    /// registered, or the engine now rejects the roster.
    #[error("setup no longer runs from the stored inputs: {0}")]
    Setup(String),
    /// Setup runs but now decides something different from what it decided
    /// when the game was created.
    #[error("setup now decides differently: {0}")]
    Fingerprint(FingerprintMismatch),
    /// A logged answer fails a replay check.
    #[error("action {seq} no longer replays: {reason}")]
    Replay {
        /// Zero-based position of the offending action in the log.
        seq: i64,
        /// The check it fails.
        reason: ReplayFailure,
    },
}

/// A loaded game: its live game and its connection to the action log.
pub struct GameSession {
    /// Stable identifier for this game (primary key in `games`).
    pub game_id: GameId,
    /// The game as it is being played.
    live: LiveGame,
    /// Connection pool for persistence.
    db: SqlitePool,
}

/// What a game's creation inputs set up: the seated, mulligan-pending state,
/// its outcome, and the events setup emitted.
struct Opened {
    state: GameState,
    outcome: EngineOutcome,
    events: Vec<Event>,
}

/// A stored game's rows as [`GameSession::read`] fetched them, before any
/// check: its `games` row and its action rows in `seq` order.
pub(crate) struct StoredGame {
    game_id: GameId,
    row: GameRow,
    actions: Vec<ActionRow>,
}

/// Why setup didn't run from a game's creation inputs.
enum SetupFailure {
    UnknownScenario(ScenarioId),
    Seating(String),
}

/// A stored game's **creation inputs**: everything its setup runs from.
struct CreationInputs {
    scenario_id: ScenarioId,
    roster: Vec<RosterEntry>,
    /// The roster index of the lead investigator.
    lead: usize,
    /// The game's RNG seed, which replaces the scenario setup's builder seed.
    seed: u64,
    interactive_acknowledge: bool,
}

impl CreationInputs {
    /// The `games` row that stores these inputs, with what setup decided from
    /// them.
    fn to_row(&self, setup_fingerprint: &SetupFingerprint) -> Result<GameRow, serde_json::Error> {
        // Seating accepted `lead`, so it indexes the roster and fits.
        let lead = i64::try_from(self.lead).map_err(|_| {
            <serde_json::Error as serde::ser::Error>::custom(format!(
                "lead {} doesn't fit its column",
                self.lead
            ))
        })?;
        Ok(GameRow {
            format_version: FORMAT_VERSION,
            scenario_id: self.scenario_id.as_str().to_owned(),
            roster: serde_json::to_string(&self.roster)?,
            lead,
            rng_seed: to_sql_integer(self.seed),
            interactive_acknowledge: self.interactive_acknowledge,
            setup_fingerprint: serde_json::to_string(setup_fingerprint)?,
        })
    }

    /// The inputs `row` stores. Its format version must already have been
    /// checked: the columns are read in the current version's shape.
    fn from_row(row: GameRow) -> Result<Self, serde_json::Error> {
        let lead = usize::try_from(row.lead).map_err(|_| {
            <serde_json::Error as serde::de::Error>::custom(format!(
                "stored lead {} is not a roster index",
                row.lead
            ))
        })?;
        Ok(Self {
            scenario_id: ScenarioId::new(row.scenario_id),
            roster: serde_json::from_str(&row.roster)?,
            lead,
            seed: from_sql_integer(row.rng_seed),
            interactive_acknowledge: row.interactive_acknowledge,
        })
    }
}

/// Run scenario setup from a game's creation inputs. `create` and `load` both
/// set a game up through here, so a reload re-runs exactly what creation ran.
fn open(registries: Registries, inputs: &CreationInputs) -> Result<Opened, SetupFailure> {
    let module = (registries.scenarios.module_for)(&inputs.scenario_id)
        .ok_or_else(|| SetupFailure::UnknownScenario(inputs.scenario_id.clone()))?;
    let mut setup = (module.setup)();
    // The scenario's setup() builds with a fixed builder seed (game-core is
    // no-I/O, so it can't source randomness itself); the game's own seed
    // replaces it, so every game gets its own shuffle and draw order.
    setup.rng = RngState::new(inputs.seed);
    setup.interactive_acknowledge = inputs.interactive_acknowledge;
    let result = engine::seat_and_open(setup, &inputs.roster, inputs.lead, registries);
    match result.outcome {
        EngineOutcome::Rejected { reason } => Err(SetupFailure::Seating(reason.to_string())),
        outcome => Ok(Opened {
            state: result.state,
            outcome,
            events: result.events,
        }),
    }
}

impl GameSession {
    /// Create a new game: seats the roster into the scenario's `setup()`
    /// state, stores the game's creation inputs and its setup fingerprint,
    /// and returns a live session at the mulligan prompt.
    ///
    /// Looks the scenario module up in `registries`, and seats and later
    /// applies against them.
    ///
    /// # Errors
    ///
    /// - [`SessionError::UnknownScenario`] if no module is registered for
    ///   `scenario_id`.
    /// - [`SessionError::Seating`] if the engine rejects the roster (empty
    ///   roster, a `lead` index past its end, unknown investigator code, unimplemented deck card, more
    ///   random basic weaknesses than the weakness pool holds, etc.).
    /// - [`SessionError::Serde`] / [`SessionError::Db`] if storing the game
    ///   fails.
    pub async fn create(
        db: SqlitePool,
        registries: Registries,
        game_id: impl Into<GameId>,
        scenario_id: ScenarioId,
        roster: Vec<RosterEntry>,
        lead: usize,
    ) -> Result<Self, SessionError> {
        let inputs = CreationInputs {
            scenario_id,
            roster,
            lead,
            seed: id::random_seed(),
            // Human play surfaces skill-test results with a Confirm-to-dismiss
            // step; the engine gates that pause on this flag (default off for
            // tests and non-interactive consumers).
            interactive_acknowledge: true,
        };
        let opened = open(registries, &inputs).map_err(|failure| match failure {
            SetupFailure::UnknownScenario(id) => SessionError::UnknownScenario(id),
            SetupFailure::Seating(reason) => SessionError::Seating(reason),
        })?;
        let row = inputs.to_row(&SetupFingerprint::of(&opened.state))?;
        let game_id = game_id.into();
        store::insert_game(&db, &game_id, &row, protocol::build_id(), unix_millis()).await?;

        Ok(Self {
            game_id,
            live: LiveGame::new(opened.state, opened.outcome, opened.events, 0, registries),
            db,
        })
    }

    /// The game as it is being played.
    #[must_use]
    pub fn live(&self) -> &LiveGame {
        &self.live
    }

    /// Submit a player action to the live game, store the answer it accepts,
    /// then commit it. Returns the [`ServerMessage::Applied`] to send every
    /// connection.
    ///
    /// A refused action stores nothing and leaves the game unchanged, and so
    /// does a failed write: the live game is committed only once the row is
    /// stored.
    ///
    /// # Errors
    ///
    /// [`ApplyError::Refused`] if the live game refuses the action, or
    /// [`ApplyError::Session`] wrapping [`SessionError::Serde`] if the answer
    /// fails to serialize or [`SessionError::Db`] if persisting it fails.
    pub async fn apply(&mut self, action: PlayerAction) -> Result<ServerMessage, ApplyError> {
        let accepted = self.live.submit(action)?;
        let row = ActionRow {
            answer: serde_json::to_string(accepted.answer()).map_err(SessionError::from)?,
            draws: to_sql_integer(accepted.draws()),
        };
        store::insert_action(
            &self.db,
            &self.game_id,
            self.live.seq(),
            &row,
            unix_millis(),
        )
        .await
        .map_err(SessionError::from)?;
        Ok(self.live.commit(accepted))
    }

    /// Rebuild a stored game against `registries`: re-run setup from its
    /// creation inputs, check what setup decided against the stored setup
    /// fingerprint, and replay its action log, running the replay checks on
    /// each logged answer. Returns `None` if no game with `game_id` exists.
    /// It is `read` followed by `rebuild`: both reads happen before setup
    /// runs, and every check runs after them.
    ///
    /// Each replayed answer overwrites `outcome`; with an empty log the
    /// session is at setup's outcome, the mulligan prompt. Setup is re-run,
    /// so a reloaded session carries the same setup events as when it was
    /// created.
    ///
    /// # Errors
    ///
    /// - [`SessionError::Unresumable`] if the game exists but can't be
    ///   rebuilt as it was played: it was stored in another format version,
    ///   setup no longer runs from its inputs or decides differently, or a
    ///   logged answer fails a replay check ([`ReplayFailure`]). Its rows are
    ///   left untouched.
    /// - [`SessionError::Db`] if a query fails, or [`SessionError::Serde`] if
    ///   a stored column or a logged answer fails to deserialize.
    pub async fn load(
        db: SqlitePool,
        registries: Registries,
        game_id: &GameId,
    ) -> Result<Option<Self>, SessionError> {
        let Some(stored) = Self::read(&db, game_id).await? else {
            return Ok(None);
        };
        Self::rebuild(db, registries, stored).map(Some)
    }

    /// The I/O half of [`GameSession::load`]: fetch a stored game's row and
    /// its action rows, unparsed. Returns `None` if no game with `game_id`
    /// exists. Runs no check, so every failure but a query's is
    /// [`GameSession::rebuild`]'s to report.
    ///
    /// # Errors
    ///
    /// [`SessionError::Db`] if a query fails.
    pub(crate) async fn read(
        db: &SqlitePool,
        game_id: &GameId,
    ) -> Result<Option<StoredGame>, SessionError> {
        let Some(row) = store::load_game(db, game_id).await? else {
            return Ok(None);
        };
        let actions = store::load_actions(db, game_id).await?;
        Ok(Some(StoredGame {
            game_id: game_id.clone(),
            row,
            actions,
        }))
    }

    /// The CPU half of [`GameSession::load`]: rebuild the live game from the
    /// rows [`GameSession::read`] fetched. It does no I/O and never awaits, so
    /// a caller can run it on a blocking thread; `db` is only kept for the
    /// session's later writes.
    ///
    /// The checks run in this order, and the first to fail is reported:
    /// format version, the stored fingerprint's deserialization, setup, the
    /// fingerprint comparison, then each logged answer's deserialization and
    /// replay.
    ///
    /// # Errors
    ///
    /// As [`GameSession::load`], except [`SessionError::Db`], which only a
    /// read reports.
    #[expect(
        clippy::result_large_err,
        reason = "the error is `load`'s, which every caller already takes by \
                  value, and it is built once per failed load; the lint only \
                  skips `load` because it is async"
    )]
    pub(crate) fn rebuild(
        db: SqlitePool,
        registries: Registries,
        stored: StoredGame,
    ) -> Result<Self, SessionError> {
        let StoredGame {
            game_id,
            row,
            actions,
        } = stored;
        let unresumable = |reason| SessionError::Unresumable {
            game_id: game_id.clone(),
            reason,
        };
        if row.format_version != FORMAT_VERSION {
            return Err(unresumable(UnresumableReason::Version {
                found: row.format_version,
            }));
        }
        let stored_fingerprint: SetupFingerprint = serde_json::from_str(&row.setup_fingerprint)?;
        let inputs = CreationInputs::from_row(row)?;
        let opened = open(registries, &inputs).map_err(|failure| {
            unresumable(UnresumableReason::Setup(match failure {
                SetupFailure::UnknownScenario(id) => {
                    format!("unknown scenario: {}", id.as_str())
                }
                SetupFailure::Seating(reason) => format!("seating rejected: {reason}"),
            }))
        })?;
        if let Some(mismatch) =
            stored_fingerprint.first_difference(&SetupFingerprint::of(&opened.state))
        {
            return Err(unresumable(UnresumableReason::Fingerprint(mismatch)));
        }

        let mut state = opened.state;
        let mut outcome = opened.outcome;
        let mut seq: i64 = 0;
        for row in actions {
            let answer: LoggedAnswer = serde_json::from_str(&row.answer)?;
            let draws = from_sql_integer(row.draws);
            // A failed check means the log does not reproduce the session it
            // recorded. Stop loudly rather than serving the prefix that did
            // replay.
            let result = replay_step(state, &outcome, &answer, draws, registries)
                .map_err(|reason| unresumable(UnresumableReason::Replay { seq, reason }))?;
            state = result.state;
            outcome = result.outcome;
            seq += 1;
        }

        Ok(Self {
            game_id,
            live: LiveGame::new(state, outcome, opened.events, seq, registries),
            db,
        })
    }
}

/// Replay one logged answer against `state`, whose last apply left `outcome`,
/// running the four replay checks in order: the prompt is the one it answered,
/// each pick finds its option, the engine accepts it, and the RNG stands at
/// the `draws` logged after it.
fn replay_step(
    state: GameState,
    outcome: &EngineOutcome,
    answer: &LoggedAnswer,
    draws: u64,
    registries: Registries,
) -> Result<ApplyResult, ReplayFailure> {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        return Err(ReplayFailure::PromptChanged {
            recorded: answer.prompt.clone(),
            now: None,
        });
    };
    let response = logged_answer::replay(request, answer)?;
    let result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput { response }),
        registries,
    );
    if let EngineOutcome::Rejected { reason } = &result.outcome {
        return Err(ReplayFailure::Rejected(reason.to_string()));
    }
    if result.state.rng.draws != draws {
        return Err(ReplayFailure::Draws {
            recorded: draws,
            now: result.state.rng.draws,
        });
    }
    Ok(result)
}

/// `bits` as `SQLite`'s signed 64-bit integer, for a `u64` column: the same
/// bits, so [`from_sql_integer`] gives `bits` back.
fn to_sql_integer(bits: u64) -> i64 {
    i64::from_ne_bytes(bits.to_ne_bytes())
}

/// The `u64` whose bits [`to_sql_integer`] stored as `integer`.
fn from_sql_integer(integer: i64) -> u64 {
    u64::from_ne_bytes(integer.to_ne_bytes())
}

/// Milliseconds since the Unix epoch, for the `created_at` and `at` columns.
/// Never replay-load-bearing.
fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}
