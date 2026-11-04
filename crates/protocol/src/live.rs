//! The **live game**: a game as it is being played, and the one decision of
//! whether a submitted answer is accepted.
//!
//! [`LiveGame`] holds the current state, its place in the action log, the
//! [`Assignment`] of investigators to accounts and the registries every apply
//! runs against. [`LiveGame::submit`] decides an answer without changing
//! anything, and [`LiveGame::commit`] advances the game by an answer it
//! accepted. It persists nothing and sends nothing: a host stores the
//! [`Accepted`] answer's logged form, then commits it and sends each
//! connection the [`Committed::applied_for`] its account.
//!
//! Every answer names the [`LogPosition`] of the prompt it answers, and
//! [`LiveGame::submit`] refuses any position but the current one before the
//! engine runs: an answer from a stale tab, a double-click, or a submit queued
//! while disconnected never resolves a prompt its sender didn't see. It then
//! refuses an account answering for an investigator it doesn't hold, which is
//! the only place accounts meet the game: the engine never sees one.
//!
//! Every message for an account is built from the state `view_for` that
//! account, which clears the RNG state, so no client can predict the chaos
//! bag or a shuffle.

use game_core::action::{Action, PlayerAction};
use game_core::engine::{self, EngineOutcome, InputRequest, Registries, Responder};
use game_core::event::Event;
use game_core::rng::RngState;
use game_core::state::investigator::InvestigatorId;
use game_core::state::{CardCode, GameState, Phase};
use serde::{Deserialize, Serialize};

use crate::logged_answer::{self, LoggedAnswer, Unrecordable};
use crate::{AccountId, Applied, Assignment, LogPosition, Refusal, ServerMessage};

/// A game as it is being played: its current state, its place in the log, who
/// answers for each investigator, and the registries it applies against.
#[derive(Debug, Clone)]
pub struct LiveGame {
    state: GameState,
    /// The outcome of the last apply, or setup's for a game with no answers
    /// yet: the prompt the game awaits, if any.
    outcome: EngineOutcome,
    /// The events scenario setup emitted, carried in every `Hello`.
    setup_events: Vec<Event>,
    /// The log position of the prompt the game awaits: the number of answers
    /// in the log, which is where the next one is stored.
    prompt: LogPosition,
    /// The account answering for each investigator.
    assignment: Assignment,
    registries: Registries,
}

/// An answer [`LiveGame::submit`] accepted: what a host persists, and what
/// [`LiveGame::commit`] advances the game to.
#[derive(Debug)]
#[must_use = "an accepted answer changes nothing until it is committed"]
pub struct Accepted {
    answer: LoggedAnswer,
    draws: u64,
    /// The game after the answer applied, which the commit installs and
    /// sends.
    state: GameState,
    events: Vec<Event>,
    outcome: EngineOutcome,
}

impl Accepted {
    /// The answer as the action log records it.
    #[must_use]
    pub fn answer(&self) -> &LoggedAnswer {
        &self.answer
    }

    /// `RngState::draws` after the answer applied: the checkpoint the log
    /// stores beside it.
    #[must_use]
    pub fn draws(&self) -> u64 {
        self.draws
    }
}

/// Why [`LiveGame::submit`] refused an answer. A host sends it to the client
/// as the wire [`Refusal`] its `From` impl maps it to.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Refused {
    /// It answered another log position than the prompt the game awaits.
    #[error("the answer is for an earlier prompt")]
    StalePrompt,
    /// It answered for an investigator the submitting account doesn't hold.
    #[error("the answer is for an investigator this account doesn't hold")]
    NotYours,
    /// The engine rejected it, for this reason.
    #[error("{0}")]
    Engine(String),
    /// The engine accepted it, but the action log can't record it, and a game
    /// never holds a move its log can't replay.
    #[error("the action log can't record this: {0}")]
    Unrecordable(Unrecordable),
}

impl From<Refused> for Refusal {
    fn from(refused: Refused) -> Self {
        match refused {
            Refused::StalePrompt => Self::StalePrompt,
            Refused::NotYours => Self::NotYours,
            Refused::Engine(reason) => Self::Engine(reason),
            unrecordable @ Refused::Unrecordable(_) => Self::Failed(unrecordable.to_string()),
        }
    }
}

impl LiveGame {
    /// The live game at `state`, whose last apply (or setup) left `outcome`,
    /// awaiting the answer at log position `prompt`, and each investigator
    /// answered for by the account `assignment` gives them to.
    #[must_use]
    pub fn new(
        state: GameState,
        outcome: EngineOutcome,
        setup_events: Vec<Event>,
        prompt: LogPosition,
        assignment: Assignment,
        registries: Registries,
    ) -> Self {
        Self {
            state,
            outcome,
            setup_events,
            prompt,
            assignment,
            registries,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// The outcome of the last apply: the prompt the game awaits, if any.
    #[must_use]
    pub fn outcome(&self) -> &EngineOutcome {
        &self.outcome
    }

    /// The events scenario setup emitted.
    #[must_use]
    pub fn setup_events(&self) -> &[Event] {
        &self.setup_events
    }

    /// The log position of the prompt the game awaits: the one a submit must
    /// name, and where the next accepted answer is stored.
    #[must_use]
    pub fn prompt(&self) -> LogPosition {
        self.prompt
    }

    /// The account answering for each investigator.
    #[must_use]
    pub fn assignment(&self) -> &Assignment {
        &self.assignment
    }

    /// The render baseline a connection signed in as `account` is sent when
    /// it attaches.
    #[must_use]
    pub fn hello_for(&self, account: AccountId) -> ServerMessage {
        ServerMessage::Hello {
            build: crate::build_id().to_string(),
            you: account,
            assigned: self.assignment.clone(),
            prompt: self.prompt(),
            state: Box::new(view_for(account, self.state.clone())),
            outcome: self.outcome.clone(),
            events: self.setup_events.clone(),
        }
    }

    /// Where the game stands, as `account` sees it on "Your games": the round
    /// and phase, and each investigator the live prompt waits on, with its
    /// holder and whether that is `account`. Nothing is waited on with no
    /// prompt live.
    #[must_use]
    pub fn summary_for(&self, account: AccountId) -> LiveSummary {
        let waited = match &self.outcome {
            EngineOutcome::AwaitingInput { request } => waited_on(request),
            _ => Vec::new(),
        };
        let waiting_on = waited
            .into_iter()
            .filter_map(|investigator| {
                let card = self.state.investigators.get(&investigator)?;
                let holder = self.assignment.holder(investigator);
                Some(WaitedOn {
                    investigator,
                    card: card.investigator_card.code.clone(),
                    holder,
                    yours: holder == Some(account),
                })
            })
            .collect();
        LiveSummary {
            round: self.state.round,
            phase: self.state.phase,
            waiting_on,
        }
    }

    /// Decide `action`, `account`'s answer to the prompt at `prompt`: check it
    /// answers the prompt the game awaits, check `account` holds the
    /// investigator it answers for, run the engine on a copy of the state,
    /// then log it. Changes nothing; an accepted answer takes effect only
    /// through [`commit`](Self::commit).
    ///
    /// # Errors
    ///
    /// In the order they are checked: [`Refused::StalePrompt`] if `prompt`
    /// isn't the position the game awaits; [`Refused::NotYours`] if `account`
    /// doesn't hold the investigator the action answers for, or the action
    /// names none; [`Refused::Engine`] if the engine rejects the action; or
    /// [`Refused::Unrecordable`] if it accepts one the log can't record.
    pub fn submit(
        &self,
        account: AccountId,
        prompt: LogPosition,
        action: PlayerAction,
    ) -> Result<Accepted, Refused> {
        if prompt != self.prompt() {
            return Err(Refused::StalePrompt);
        }
        if !answering(&action).is_some_and(|inv| self.assignment.holds(account, inv)) {
            return Err(Refused::NotYours);
        }
        let answer = self.record(&action);
        let result = engine::apply(self.state.clone(), Action::Player(action), self.registries);
        if let EngineOutcome::Rejected { reason } = result.outcome {
            return Err(Refused::Engine(reason.into_owned()));
        }
        let answer = answer.map_err(Refused::Unrecordable)?;
        Ok(Accepted {
            answer,
            draws: result.state.rng.draws,
            state: result.state,
            events: result.events,
            outcome: result.outcome,
        })
    }

    /// Advance the game by an answer [`submit`](Self::submit) accepted, and
    /// hand back the [`Committed`] answer, from which each connection is sent
    /// the [`Applied`] for its account. A host that persists the log commits
    /// only once the answer is stored.
    ///
    /// `accepted` must come from `submit` on this game in its current state:
    /// the type can't tie the two together, so committing a stale answer
    /// installs that answer's state.
    pub fn commit(&mut self, accepted: Accepted) -> Committed {
        let answered = self.prompt;
        self.prompt = answered.next();
        self.state = accepted.state.clone();
        self.outcome = accepted.outcome.clone();
        Committed(Applied {
            answered,
            by: accepted.answer.investigator,
            response: accepted.answer.response,
            prompt: self.prompt,
            state: Box::new(accepted.state),
            events: accepted.events,
            outcome: accepted.outcome,
        })
    }

    /// Log `action` as the answer to the prompt the game awaits.
    fn record(&self, action: &PlayerAction) -> Result<LoggedAnswer, Unrecordable> {
        let PlayerAction::ResolveInput {
            investigator,
            response,
        } = action
        else {
            return Err(Unrecordable::NotAnAnswer(action.clone()));
        };
        let EngineOutcome::AwaitingInput { request, .. } = &self.outcome else {
            return Err(Unrecordable::NoPrompt);
        };
        logged_answer::record(request, *investigator, response)
    }
}

/// An answer [`LiveGame::commit`] installed, the same for every account: a host
/// shares one among its connections, and each sends
/// [`applied_for`](Self::applied_for) its own account.
///
/// It holds the [`Applied`] before any account's view is cut from its state.
#[derive(Debug)]
pub struct Committed(Applied);

impl Committed {
    /// The [`Applied`] a connection signed in as `account` is sent.
    #[must_use]
    pub fn applied_for(&self, account: AccountId) -> Applied {
        let mut applied = self.0.clone();
        applied.state = Box::new(view_for(account, *applied.state));
        applied
    }
}

/// A live game's place summarised for one account: what "Your games" shows
/// of a started game. Read off the game each time it is asked for, never
/// stored (ADR 0022).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSummary {
    /// The round, from 1.
    pub round: u32,
    /// The phase.
    pub phase: Phase,
    /// Each investigator the live prompt waits on, in the order the prompt
    /// names them; empty with no prompt live.
    pub waiting_on: Vec<WaitedOn>,
}

impl LiveSummary {
    /// Whether the prompt waits on an investigator the account holds.
    #[must_use]
    pub fn needs_you(&self) -> bool {
        self.waiting_on.iter().any(|waited| waited.yours)
    }

    /// The waited-on investigators the account holds.
    pub fn yours(&self) -> impl Iterator<Item = &WaitedOn> {
        self.waiting_on.iter().filter(|waited| waited.yours)
    }

    /// The accounts holding a waited-on investigator, each once, in the
    /// order their investigators are waited on.
    #[must_use]
    pub fn waiting_for(&self) -> Vec<AccountId> {
        let mut accounts = Vec::new();
        for holder in self.waiting_on.iter().filter_map(|waited| waited.holder) {
            if !accounts.contains(&holder) {
                accounts.push(holder);
            }
        }
        accounts
    }
}

/// One investigator a live prompt waits on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitedOn {
    /// The investigator.
    pub investigator: InvestigatorId,
    /// Its investigator card, which names it.
    pub card: CardCode,
    /// The account holding it, if one does.
    pub holder: Option<AccountId>,
    /// Whether the account the summary is for holds it.
    pub yours: bool,
}

/// Who `request` waits on, read off its [`Responder`] as the board's status
/// bar reads it: the one investigator a prompt for an investigator or for the
/// lead names, or in a window several answer, each investigator holding an
/// option.
fn waited_on(request: &InputRequest) -> Vec<InvestigatorId> {
    match request.responder {
        Responder::Investigator(id) | Responder::Lead(id) => vec![id],
        Responder::Several => request.holders(),
        // `Responder` is non-exhaustive; a kind this build does not know
        // names nobody it can show.
        _ => Vec::new(),
    }
}

/// The investigator `action` answers for, if it names one.
fn answering(action: &PlayerAction) -> Option<InvestigatorId> {
    match action {
        PlayerAction::ResolveInput { investigator, .. } => Some(*investigator),
        _ => None,
    }
}

/// `state` as `account` may see it: the one place a message for an account
/// is cut down from the game, so hidden information has one place to change.
///
/// Today it clears the RNG state, which would let a client predict every
/// chaos-token pull and shuffle, and nothing else, so every account sees the
/// same state.
fn view_for(_account: AccountId, mut state: GameState) -> GameState {
    state.rng = RngState::new(0);
    state
}
