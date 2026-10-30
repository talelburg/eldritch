//! `GameSession`: the host wrapper pairing the pure engine with the
//! `SQLite` action log.
//!
//! A **stored game** is its creation inputs (scenario, roster, lead, RNG seed,
//! the `interactive_acknowledge` flag) plus an ordered log of its answers
//! (ADR 0022). No engine state is stored. The live session keeps the derived
//! state in memory and appends each accepted answer to the log as a
//! [`LoggedAnswer`] with the RNG checkpoint after it; [`GameSession::load`]
//! re-runs setup from the inputs, checks it against the stored
//! [`SetupFingerprint`], and replays the log, checking each answer, reproducing
//! state bit-for-bit. A stored game that fails a check is an **unresumable
//! game**: the load fails with [`SessionError::Unresumable`] and its rows are
//! kept.

use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{Action, PlayerAction, RosterEntry};
use game_core::engine::{self, ApplyResult, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::rng::RngState;
use game_core::scenario::ScenarioId;
use game_core::state::game_state::GameState;
use protocol::GameId;
use sqlx::SqlitePool;

use crate::fingerprint::{FingerprintMismatch, SetupFingerprint};
use crate::id;
use crate::logged_answer::{self, LoggedAnswer, ReplayFailure};
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

/// A live game: derived state plus its connection to the action log.
pub struct GameSession {
    /// Stable identifier for this game (primary key in `games`).
    pub game_id: GameId,
    /// Current derived state.
    pub state: GameState,
    /// Outcome of the most recent apply, or — for a game with no actions yet —
    /// setup's outcome (the setup mulligan prompt, i.e. `AwaitingInput`).
    pub outcome: EngineOutcome,
    /// Events emitted during `seat_and_open`, surfaced to newly-connecting
    /// clients via `ServerMessage::Hello` so the event log can show opening
    /// draws, shuffles, and the weakness set-aside. [`load`](Self::load)
    /// re-runs setup, so a session reloaded from the DB carries the same list.
    pub setup_events: Vec<Event>,
    /// Next sequence number to assign to a persisted action — equal to
    /// the number of actions already in the log.
    seq: i64,
    /// Connection pool for persistence.
    db: SqlitePool,
    /// The registries every apply runs against.
    registries: Registries,
}

/// What a game's creation inputs set up: the seated, mulligan-pending state,
/// its outcome, and the events setup emitted.
struct Opened {
    state: GameState,
    outcome: EngineOutcome,
    events: Vec<Event>,
}

/// Why setup didn't run from a game's creation inputs.
enum SetupFailure {
    UnknownScenario(ScenarioId),
    Seating(String),
}

/// Run scenario setup from a game's creation inputs. `create` and `load` both
/// set a game up through here, so a reload re-runs exactly what creation ran.
fn open(
    registries: Registries,
    scenario_id: &ScenarioId,
    roster: &[RosterEntry],
    lead: usize,
    seed: u64,
    interactive_acknowledge: bool,
) -> Result<Opened, SetupFailure> {
    let module = (registries.scenarios.module_for)(scenario_id)
        .ok_or_else(|| SetupFailure::UnknownScenario(scenario_id.clone()))?;
    let mut setup = (module.setup)();
    // The scenario's setup() builds with a fixed builder seed (game-core is
    // no-I/O, so it can't source randomness itself); the game's own seed
    // replaces it, so every game gets its own shuffle and draw order.
    setup.rng = RngState::new(seed);
    setup.interactive_acknowledge = interactive_acknowledge;
    let result = engine::seat_and_open(setup, roster, lead, registries);
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
        let seed = id::random_seed();
        // Human play surfaces skill-test results with a Confirm-to-dismiss
        // step; the engine gates that pause on this flag (default off for tests
        // and non-interactive consumers).
        let interactive_acknowledge = true;
        let opened = open(
            registries,
            &scenario_id,
            &roster,
            lead,
            seed,
            interactive_acknowledge,
        )
        .map_err(|failure| match failure {
            SetupFailure::UnknownScenario(id) => SessionError::UnknownScenario(id),
            SetupFailure::Seating(reason) => SessionError::Seating(reason),
        })?;
        // Seating accepted `lead`, so it indexes the roster and fits.
        let lead = i64::try_from(lead)
            .map_err(|_| SessionError::Seating(format!("lead {lead} is past the roster")))?;
        let row = GameRow {
            format_version: FORMAT_VERSION,
            scenario_id: scenario_id.as_str().to_owned(),
            roster: serde_json::to_string(&roster)?,
            lead,
            rng_seed: i64::from_ne_bytes(seed.to_ne_bytes()),
            interactive_acknowledge,
            setup_fingerprint: serde_json::to_string(&SetupFingerprint::of(&opened.state))?,
        };
        let game_id = game_id.into();
        store::insert_game(
            &db,
            &game_id,
            &row,
            protocol::build_id(),
            &unix_millis().to_string(),
        )
        .await?;

        Ok(Self {
            game_id,
            state: opened.state,
            outcome: opened.outcome,
            setup_events: opened.events,
            seq: 0,
            db,
            registries,
        })
    }

    /// Apply a player action: validate via the engine, log the answer it gives
    /// on acceptance, and advance the in-memory state.
    ///
    /// A rejected action persists nothing and leaves the state
    /// unchanged (the engine guarantees this); the returned
    /// [`EngineOutcome`] carries the rejection reason. An action the engine
    /// accepts but the log can't record ([`logged_answer::record`]) is
    /// rejected the same way, since a game must never hold a move its log
    /// can't replay.
    ///
    /// # Errors
    ///
    /// [`SessionError::Serde`] if the answer fails to serialize, or
    /// [`SessionError::Db`] if persisting it fails. An engine *rejection* is
    /// not an error — it is returned in the `EngineOutcome`.
    pub async fn apply(
        &mut self,
        action: PlayerAction,
    ) -> Result<(Vec<Event>, EngineOutcome), SessionError> {
        let result = engine::apply(
            self.state.clone(),
            Action::Player(action.clone()),
            self.registries,
        );
        if matches!(result.outcome, EngineOutcome::Rejected { .. }) {
            return Ok((result.events, result.outcome));
        }
        let answer = match self.record(&action) {
            Ok(answer) => answer,
            Err(reason) => {
                return Ok((
                    Vec::new(),
                    EngineOutcome::Rejected {
                        reason: Cow::Owned(format!("the action log can't record this: {reason}")),
                    },
                ));
            }
        };

        let row = ActionRow {
            answer: serde_json::to_string(&answer)?,
            draws: i64::from_ne_bytes(result.state.rng.draws.to_ne_bytes()),
        };
        store::insert_action(&self.db, &self.game_id, self.seq, &row, unix_millis()).await?;
        self.seq += 1;
        self.state = result.state;
        self.outcome = result.outcome.clone();

        Ok((result.events, result.outcome))
    }

    /// Log `action` as the answer to the prompt the session awaits.
    fn record(&self, action: &PlayerAction) -> Result<LoggedAnswer, String> {
        let PlayerAction::ResolveInput { response } = action else {
            return Err(format!("{action:?} answers no prompt"));
        };
        let EngineOutcome::AwaitingInput { request, .. } = &self.outcome else {
            return Err("the game awaits no answer".to_owned());
        };
        logged_answer::record(request, response).map_err(|unrecordable| unrecordable.to_string())
    }

    /// Rebuild a stored game against `registries`: re-run setup from its
    /// creation inputs, check what setup decided against the stored setup
    /// fingerprint, and replay its action log, running the replay checks on
    /// each logged answer. Returns `None` if no game with `game_id` exists.
    ///
    /// Each replayed answer overwrites `outcome`; with an empty log the
    /// session is at setup's outcome, the mulligan prompt.
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
        let Some(row) = store::load_game(&db, game_id).await? else {
            return Ok(None);
        };
        let unresumable = |reason| SessionError::Unresumable {
            game_id: game_id.clone(),
            reason,
        };
        if row.format_version != FORMAT_VERSION {
            return Err(unresumable(UnresumableReason::Version {
                found: row.format_version,
            }));
        }
        let roster: Vec<RosterEntry> = serde_json::from_str(&row.roster)?;
        let stored_fingerprint: SetupFingerprint = serde_json::from_str(&row.setup_fingerprint)?;
        let lead = usize::try_from(row.lead).map_err(|_| {
            unresumable(UnresumableReason::Setup(format!(
                "stored lead {} is not a roster index",
                row.lead
            )))
        })?;
        let opened = open(
            registries,
            &ScenarioId::new(row.scenario_id),
            &roster,
            lead,
            u64::from_ne_bytes(row.rng_seed.to_ne_bytes()),
            row.interactive_acknowledge,
        )
        .map_err(|failure| {
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
        for row in store::load_actions(&db, game_id).await? {
            let answer: LoggedAnswer = serde_json::from_str(&row.answer)?;
            let draws = u64::from_ne_bytes(row.draws.to_ne_bytes());
            // A failed check means the log does not reproduce the session it
            // recorded. Stop loudly rather than serving the prefix that did
            // replay.
            let result = replay(state, &outcome, &answer, draws, registries)
                .map_err(|reason| unresumable(UnresumableReason::Replay { seq, reason }))?;
            state = result.state;
            outcome = result.outcome;
            seq += 1;
        }

        Ok(Some(Self {
            game_id: game_id.clone(),
            state,
            outcome,
            setup_events: opened.events,
            seq,
            db,
            registries,
        }))
    }
}

/// Replay one logged answer against `state`, whose last apply left `outcome`,
/// running the four replay checks in order: the prompt is the one it answered,
/// each pick finds its option, the engine accepts it, and the RNG stands at
/// the `draws` logged after it.
fn replay(
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

/// Milliseconds since the Unix epoch, for the `created_at` and `at` columns.
/// Never replay-load-bearing.
fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}
