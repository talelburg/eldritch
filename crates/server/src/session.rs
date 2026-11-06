//! `GameSession`: the host wrapper pairing a [`LiveGame`] with the `SQLite`
//! action log.
//!
//! A **stored game** is its creation inputs (scenario, roster, lead, RNG seed,
//! the `interactive_acknowledge` flag) plus an ordered log of its answers
//! (ADR 0022), the account that created it, and beside them its assignment:
//! which account holds each investigator (ADR 0024). No engine state is
//! stored. The session keeps the live game in memory and appends each answer
//! it accepts to the log as a [`LoggedAnswer`] with the RNG checkpoint after
//! it, before committing it; [`GameSession::load`] re-runs setup from the
//! inputs, checks it against the stored [`SetupFingerprint`], and replays the
//! log, checking each answer, reproducing state bit-for-bit. A stored game that
//! fails a check is an **unresumable game**: the load fails with
//! [`SessionError::Unresumable`] and its rows are kept.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{Action, PlayerAction, RosterEntry};
use game_core::engine::{self, ApplyResult, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::rng::RngState;
use game_core::scenario::ScenarioId;
use game_core::state::game_state::GameState;
use game_core::state::InvestigatorId;
use protocol::live::{Committed, LiveGame, Refused};
use protocol::lobby::{Member, Start};
use protocol::logged_answer::{self, LoggedAnswer, ReplayFailure};
use protocol::{
    AccountId, Assignment, GameId, LogPosition, ReassignRefusal, ServerMessage, UnresumableCause,
};
use sqlx::{SqliteConnection, SqlitePool};

use crate::fingerprint::{FingerprintMismatch, SetupFingerprint};
use crate::store::{self, ActionRow, GameRow};
use crate::{id, lobby};

/// The version of the stored shape: the creation inputs and a log entry. Bumped
/// whenever either changes; a game stored under any other version is
/// unresumable ([`UnresumableReason::Version`]).
pub const FORMAT_VERSION: i64 = 3;

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
    /// The assignment a game was to be created with doesn't hold exactly the
    /// investigators seating seated: one would have no holder, so nobody could
    /// answer its prompts, or a holder would hold an investigator not in the
    /// game.
    #[error("the assignment covers {assigned:?}, but seating seated {seated:?}")]
    AssignmentMismatch {
        /// The investigators seating seated, in id order.
        seated: Vec<InvestigatorId>,
        /// The investigators the assignment gives a holder, in id order.
        assigned: Vec<InvestigatorId>,
    },
    /// A stored game exists but this build can't rebuild it as it was played.
    ///
    /// Failing is deliberate: a partially or differently reconstructed session
    /// is a silently wrong board. The game's rows are left as they are, so
    /// rolling the deploy back recovers it.
    #[error("game {} is unresumable: {reason}", .game_id.as_str())]
    Unresumable {
        /// The game that failed to load.
        game_id: GameId,
        /// Why it can't be resumed. Boxed: the reason is the largest payload
        /// by far, and every other variant would otherwise carry its size.
        reason: Box<UnresumableReason>,
    },
}

/// Why [`GameSession::apply`] didn't apply an action. The socket sends the
/// submitting client the wire [`Refusal`](protocol::Refusal) it maps to.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApplyError {
    /// The live game refused it. Nothing was stored.
    #[error(transparent)]
    Refused(#[from] Refused),
    /// The live game accepted it, but storing it failed. Nothing was
    /// committed.
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// Why [`GameSession::reassign`] didn't reassign an investigator. A host
/// sends the account that asked the wire [`Refusal`](protocol::Refusal) it
/// maps to.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReassignError {
    /// The live game refused it. Nothing was stored.
    #[error(transparent)]
    Refused(#[from] ReassignRefusal),
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
    /// The game records no assignment, so nobody holds its investigators.
    /// Every game stored before the assignment was stored beside it has none,
    /// and nothing falls back to the creator holding everyone (ADR 0024).
    #[error("the game records no assignment")]
    NoAssignment,
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
        seq: u32,
        /// The check it fails.
        reason: ReplayFailure,
    },
}

impl UnresumableReason {
    /// The reason as "Your games" lists it.
    #[must_use]
    pub fn cause(&self) -> UnresumableCause {
        match self {
            Self::Version { .. } => UnresumableCause::Version,
            Self::NoAssignment => UnresumableCause::NoAssignment,
            Self::Setup(message) => UnresumableCause::Setup(message.clone()),
            Self::Fingerprint(_) => UnresumableCause::Fingerprint,
            Self::Replay { seq, reason } => UnresumableCause::Replay {
                seq: *seq,
                cause: reason.into(),
            },
        }
    }
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

/// A new game [`GameSession::set_up`] seated and checked, not yet stored:
/// [store](Self::store) it, then [open its session](Self::into_session).
pub struct NewGame {
    game_id: GameId,
    /// The account that created it, which its row stores.
    creator: AccountId,
    row: GameRow,
    opened: Opened,
    assignment: Assignment,
    registries: Registries,
}

impl NewGame {
    /// Store the game's row and its assignment on `db`, normally a
    /// transaction the caller commits.
    ///
    /// # Errors
    ///
    /// [`SessionError::Db`] if a write fails.
    pub async fn store(&self, db: &mut SqliteConnection) -> Result<(), SessionError> {
        store::insert_game(
            db,
            &self.game_id,
            &self.row,
            protocol::build_id(),
            unix_millis(),
        )
        .await?;
        store::insert_assignment(db, &self.game_id, &self.assignment).await?;
        Ok(())
    }

    /// The live session of the game, at the mulligan prompt, once it is
    /// [stored](Self::store), with `members` in the order they joined, writing
    /// its later answers to `db`.
    #[must_use]
    pub fn into_session(self, db: SqlitePool, members: Vec<Member>) -> GameSession {
        GameSession {
            game_id: self.game_id,
            live: LiveGame::new(
                self.opened.state,
                self.opened.outcome,
                self.opened.events,
                LogPosition::default(),
                self.assignment,
                self.creator,
                self.registries,
            )
            .with_members(members),
            db,
        }
    }
}

/// A stored game's rows as [`GameSession::read`] fetched them, before any
/// check: its `games` row, its assignment rows, its members in the order they
/// joined and its action rows in `seq` order.
pub(crate) struct StoredGame {
    game_id: GameId,
    row: GameRow,
    assignment: Assignment,
    members: Vec<Member>,
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
    /// The `games` row that stores these inputs for a game `creator` created,
    /// with what setup decided from them.
    fn to_row(
        &self,
        creator: AccountId,
        setup_fingerprint: &SetupFingerprint,
    ) -> Result<GameRow, serde_json::Error> {
        // Seating accepted `lead`, so it indexes the roster and fits.
        let lead = i64::try_from(self.lead).map_err(|_| {
            <serde_json::Error as serde::ser::Error>::custom(format!(
                "lead {} doesn't fit its column",
                self.lead
            ))
        })?;
        Ok(GameRow {
            format_version: FORMAT_VERSION,
            creator: Some(creator),
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

/// Run scenario setup from a game's creation inputs. `set_up` and `load` both
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
    /// Set up a new game for `creator`: seat `start`'s roster into its
    /// scenario's `setup()` state and check that `start`'s assignment holds
    /// exactly the seated investigators, storing nothing. The returned
    /// [`NewGame`] carries the game's creation inputs, its setup fingerprint,
    /// its creator and the assignment. The caller [stores](NewGame::store) it
    /// inside a transaction of its own, so it can write other rows in the same
    /// one, as the lobby's Start deletes the lobby's; then
    /// [`NewGame::into_session`] gives the live session at the mulligan prompt.
    ///
    /// Looks the scenario module up in `registries`, and seats and later
    /// applies against them.
    ///
    /// # Errors
    ///
    /// - [`SessionError::UnknownScenario`] if no module is registered for
    ///   `start`'s scenario.
    /// - [`SessionError::Seating`] if the engine rejects the roster (empty
    ///   roster, a lead index past its end, unknown investigator code,
    ///   unimplemented deck card, more random basic weaknesses than the
    ///   weakness pool holds, etc.).
    /// - [`SessionError::AssignmentMismatch`] if the assignment doesn't give
    ///   exactly the seated investigators a holder.
    /// - [`SessionError::Serde`] if the creation inputs or the setup
    ///   fingerprint fail to serialize.
    pub fn set_up(
        registries: Registries,
        game_id: GameId,
        creator: AccountId,
        start: Start,
    ) -> Result<NewGame, SessionError> {
        let Start {
            scenario_id,
            roster,
            lead,
            assignment,
        } = start;
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
        let seated: Vec<InvestigatorId> = opened.state.investigators.keys().copied().collect();
        let assigned: Vec<InvestigatorId> = assignment
            .iter()
            .map(|(investigator, _)| investigator)
            .collect();
        if assigned != seated {
            return Err(SessionError::AssignmentMismatch { seated, assigned });
        }
        let row = inputs.to_row(creator, &SetupFingerprint::of(&opened.state))?;
        Ok(NewGame {
            game_id,
            creator,
            row,
            opened,
            assignment,
            registries,
        })
    }

    /// The game as it is being played.
    #[must_use]
    pub fn live(&self) -> &LiveGame {
        &self.live
    }

    /// Add `member` to the live game's members, unless their account is one
    /// already: what a host does once it has stored the membership. Returns
    /// whether it was added.
    pub fn join(&mut self, member: Member) -> bool {
        self.live.join(member)
    }

    /// Submit a player action, `account`'s answer to the prompt at `prompt`,
    /// to the live game, store the answer it accepts, then commit it. Returns
    /// the [`Committed`] answer, from which each connection is sent the
    /// `Applied` for its own account.
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
    pub async fn apply(
        &mut self,
        account: AccountId,
        prompt: LogPosition,
        action: PlayerAction,
    ) -> Result<Committed, ApplyError> {
        let accepted = self.live.submit(account, prompt, action)?;
        let row = ActionRow {
            answer: serde_json::to_string(accepted.answer()).map_err(SessionError::from)?,
            draws: to_sql_integer(accepted.draws()),
        };
        store::insert_action(
            &self.db,
            &self.game_id,
            self.live.prompt().0,
            &row,
            unix_millis(),
        )
        .await
        .map_err(SessionError::from)?;
        Ok(self.live.commit(accepted))
    }

    /// **Reassign** `investigator` to the member `to`, as `by` asks: the live
    /// game decides it, the new holder is stored, then the live game commits
    /// it. Returns the [`ServerMessage::Assigned`] every connection is sent.
    ///
    /// A refused reassignment stores nothing and leaves the game unchanged,
    /// and so does a failed write. Nothing reaches the action log.
    ///
    /// # Errors
    ///
    /// [`ReassignError::Refused`] if the live game refuses it, or
    /// [`ReassignError::Session`] wrapping [`SessionError::Db`] if storing the
    /// new holder fails.
    pub async fn reassign(
        &mut self,
        by: AccountId,
        investigator: InvestigatorId,
        to: AccountId,
    ) -> Result<ServerMessage, ReassignError> {
        let reassigned = self.live.reassign(by, investigator, to)?;
        store::update_holder(
            &self.db,
            &self.game_id,
            reassigned.investigator(),
            reassigned.to(),
        )
        .await
        .map_err(SessionError::from)?;
        Ok(self.live.commit_reassignment(reassigned))
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
    ///   it records no assignment, setup no longer runs from its inputs or
    ///   decides differently, or a logged answer fails a replay check
    ///   ([`ReplayFailure`]). Its rows are left untouched.
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

    /// The I/O half of [`GameSession::load`]: fetch a stored game's row, its
    /// assignment rows, its members and its action rows, unparsed. Returns
    /// `None` if no game with `game_id` exists. Runs no check, so every
    /// failure but a query's is [`GameSession::rebuild`]'s to report.
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
        let assignment = store::load_assignment(db, game_id).await?;
        let members = lobby::named_members(db, game_id).await?;
        let actions = store::load_actions(db, game_id).await?;
        Ok(Some(StoredGame {
            game_id: game_id.clone(),
            row,
            assignment,
            members,
            actions,
        }))
    }

    /// The CPU half of [`GameSession::load`]: rebuild the live game from the
    /// rows [`GameSession::read`] fetched. It does no I/O and never awaits, so
    /// a caller can run it on a blocking thread; `db` is only kept for the
    /// session's later writes.
    ///
    /// The checks run in this order, and the first to fail is reported:
    /// format version, the assignment, the creator, the stored fingerprint's
    /// deserialization, setup, the fingerprint comparison, then each logged
    /// answer's deserialization and replay.
    ///
    /// # Errors
    ///
    /// As [`GameSession::load`], except [`SessionError::Db`], which only a
    /// read reports.
    pub(crate) fn rebuild(
        db: SqlitePool,
        registries: Registries,
        stored: StoredGame,
    ) -> Result<Self, SessionError> {
        let StoredGame {
            game_id,
            row,
            assignment,
            members,
            actions,
        } = stored;
        let unresumable = |reason| SessionError::Unresumable {
            game_id: game_id.clone(),
            reason: Box::new(reason),
        };
        if row.format_version != FORMAT_VERSION {
            return Err(unresumable(UnresumableReason::Version {
                found: row.format_version,
            }));
        }
        if assignment.is_empty() {
            return Err(unresumable(UnresumableReason::NoAssignment));
        }
        // Every game of this format version was stored with its creator, so a
        // row without one is corrupt, as a column that fails to parse is.
        let creator = row.creator.ok_or_else(|| {
            <serde_json::Error as serde::de::Error>::custom(format!(
                "game {} of format version {FORMAT_VERSION} stores no creator",
                game_id.as_str()
            ))
        })?;
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
        // Positions are counted in `u32`, as "Your games" reports them: a
        // log too long for one is refused here, so the count never overflows.
        u32::try_from(actions.len()).map_err(|_| {
            <serde_json::Error as serde::de::Error>::custom(format!(
                "a log of {} answers is too long to replay",
                actions.len()
            ))
        })?;
        let mut seq: u32 = 0;
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
            live: LiveGame::new(
                state,
                outcome,
                opened.events,
                LogPosition(i64::from(seq)),
                assignment,
                creator,
                registries,
            )
            .with_members(members),
            db,
        })
    }
}

/// Replay one logged answer against `state`, whose last apply left `outcome`,
/// running the four replay checks in order: the prompt is the one it answered,
/// each pick finds its option, the engine accepts it (its responder check
/// included, against the logged investigator), and the RNG stands at
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
    let action = logged_answer::replay(request, answer)?;
    let result = engine::apply(state, Action::Player(action), registries);
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
pub(crate) fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}
