//! The **live game**: a game as it is being played, and the one decision of
//! whether a submitted answer is accepted.
//!
//! [`LiveGame`] holds the current state, its place in the action log and the
//! registries every apply runs against. [`LiveGame::submit`] decides an answer
//! without changing anything, and [`LiveGame::commit`] advances the game by an
//! answer it accepted. It persists nothing and sends nothing: a host stores
//! the [`Accepted`] answer's logged form, then commits it and sends the
//! [`ServerMessage::Applied`] that comes back.
//!
//! Every answer names the [`LogPosition`] of the prompt it answers, and
//! [`LiveGame::submit`] refuses any position but the current one before the
//! engine runs: an answer from a stale tab, a double-click, or a submit queued
//! while disconnected never resolves a prompt its sender didn't see.

use game_core::action::{Action, PlayerAction};
use game_core::engine::{self, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::state::GameState;

use crate::logged_answer::{self, LoggedAnswer, Unrecordable};
use crate::{Applied, LogPosition, Refusal, ServerMessage};

/// A game as it is being played: its current state, its place in the log, and
/// the registries it applies against.
#[derive(Debug, Clone)]
pub struct LiveGame {
    state: GameState,
    /// The outcome of the last apply, or setup's for a game with no answers
    /// yet: the prompt the game awaits, if any.
    outcome: EngineOutcome,
    /// The events scenario setup emitted, carried in every `Hello`.
    setup_events: Vec<Event>,
    /// The number of answers in the log, which is the position the next one
    /// is stored at.
    seq: i64,
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
            Refused::Engine(reason) => Self::Engine(reason),
            unrecordable @ Refused::Unrecordable(_) => Self::Failed(unrecordable.to_string()),
        }
    }
}

impl LiveGame {
    /// The live game at `state`, whose last apply (or setup) left `outcome`,
    /// with `seq` answers already in its log.
    #[must_use]
    pub fn new(
        state: GameState,
        outcome: EngineOutcome,
        setup_events: Vec<Event>,
        seq: i64,
        registries: Registries,
    ) -> Self {
        Self {
            state,
            outcome,
            setup_events,
            seq,
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

    /// The number of answers in the log, which is the position the next
    /// accepted answer is stored at.
    #[must_use]
    pub fn seq(&self) -> i64 {
        self.seq
    }

    /// The log position of the prompt the game awaits: the one a submit must
    /// name.
    #[must_use]
    pub fn prompt(&self) -> LogPosition {
        LogPosition(self.seq)
    }

    /// The render baseline a connection is sent when it attaches.
    #[must_use]
    pub fn hello(&self) -> ServerMessage {
        ServerMessage::Hello {
            prompt: self.prompt(),
            state: Box::new(self.state.clone()),
            outcome: self.outcome.clone(),
            events: self.setup_events.clone(),
        }
    }

    /// Decide `action`, the answer to the prompt at `prompt`: check it answers
    /// the prompt the game awaits, run the engine on a copy of the state, then
    /// log it. Changes nothing; an accepted answer takes effect only through
    /// [`commit`](Self::commit).
    ///
    /// # Errors
    ///
    /// [`Refused::StalePrompt`] if `prompt` isn't the position the game
    /// awaits, checked before the engine runs; [`Refused::Engine`] if the
    /// engine rejects the action; or [`Refused::Unrecordable`] if it accepts
    /// one the log can't record.
    pub fn submit(&self, prompt: LogPosition, action: PlayerAction) -> Result<Accepted, Refused> {
        if prompt != self.prompt() {
            return Err(Refused::StalePrompt);
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
    /// hand back the [`ServerMessage::Applied`] every connection is sent. A
    /// host that persists the log commits only once the answer is stored.
    ///
    /// `accepted` must come from `submit` on this game in its current state:
    /// the type can't tie the two together, so committing a stale answer
    /// installs that answer's state.
    pub fn commit(&mut self, accepted: Accepted) -> ServerMessage {
        let answered = self.prompt();
        let applied = ServerMessage::Applied(Applied {
            answered,
            prompt: answered.next(),
            state: Box::new(accepted.state.clone()),
            events: accepted.events,
            outcome: accepted.outcome.clone(),
        });
        self.state = accepted.state;
        self.outcome = accepted.outcome;
        self.seq += 1;
        applied
    }

    /// Log `action` as the answer to the prompt the game awaits.
    fn record(&self, action: &PlayerAction) -> Result<LoggedAnswer, Unrecordable> {
        let PlayerAction::ResolveInput { response } = action else {
            return Err(Unrecordable::NotAnAnswer(action.clone()));
        };
        let EngineOutcome::AwaitingInput { request, .. } = &self.outcome else {
            return Err(Unrecordable::NoPrompt);
        };
        logged_answer::record(request, response)
    }
}
