//! The **live game**: a game as it is being played, and the one decision of
//! whether a submitted answer is accepted.
//!
//! [`LiveGame`] holds the current state, its place in the action log and the
//! registries every apply runs against. [`LiveGame::submit`] decides an answer
//! without changing anything, and [`LiveGame::commit`] advances the game by an
//! answer it accepted. It persists nothing and sends nothing: a host stores
//! the [`Accepted`] answer's logged form, then commits it and sends the
//! [`ServerMessage::Applied`] that comes back.

use game_core::action::{Action, PlayerAction};
use game_core::engine::{self, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::state::GameState;

use crate::logged_answer::{self, LoggedAnswer, Unrecordable};
use crate::ServerMessage;

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
#[derive(Debug, Clone)]
#[must_use = "an accepted answer changes nothing until it is committed"]
pub struct Accepted {
    answer: LoggedAnswer,
    draws: u64,
    applied: ServerMessage,
    state: GameState,
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

    /// The [`ServerMessage::Applied`] every connection is sent once the answer
    /// is committed.
    #[must_use]
    pub fn applied(&self) -> &ServerMessage {
        &self.applied
    }
}

/// Why [`LiveGame::submit`] refused an answer. Its `Display` is the reason a
/// client is sent in [`ServerMessage::Rejected`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// The engine rejected it, for this reason.
    #[error("{0}")]
    Engine(String),
    /// The engine accepted it, but the action log can't record it, and a game
    /// never holds a move its log can't replay.
    #[error("the action log can't record this: {0}")]
    Unrecordable(Unrecordable),
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

    /// The render baseline a connection is sent when it attaches.
    #[must_use]
    pub fn hello(&self) -> ServerMessage {
        ServerMessage::Hello {
            state: Box::new(self.state.clone()),
            outcome: self.outcome.clone(),
            events: self.setup_events.clone(),
        }
    }

    /// Decide `action`: run the engine on a copy of the state, then log it as
    /// the answer to the prompt the game awaits. Changes nothing; an accepted
    /// answer takes effect only through [`commit`](Self::commit).
    ///
    /// # Errors
    ///
    /// [`Refusal::Engine`] if the engine rejects the action, or
    /// [`Refusal::Unrecordable`] if it accepts one the log can't record.
    pub fn submit(&self, action: PlayerAction) -> Result<Accepted, Refusal> {
        let answer = self.record(&action);
        let result = engine::apply(self.state.clone(), Action::Player(action), self.registries);
        if let EngineOutcome::Rejected { reason } = result.outcome {
            return Err(Refusal::Engine(reason.into_owned()));
        }
        let answer = answer.map_err(Refusal::Unrecordable)?;
        Ok(Accepted {
            answer,
            draws: result.state.rng.draws,
            applied: ServerMessage::Applied {
                state: Box::new(result.state.clone()),
                events: result.events,
                outcome: result.outcome.clone(),
            },
            state: result.state,
            outcome: result.outcome,
        })
    }

    /// Advance the game by an answer [`submit`](Self::submit) accepted, and
    /// hand back its [`ServerMessage::Applied`]. A host that persists the log
    /// commits only once the answer is stored.
    ///
    /// `accepted` must come from `submit` on this game in its current state:
    /// the type can't tie the two together, so committing a stale answer
    /// installs that answer's state.
    pub fn commit(&mut self, accepted: Accepted) -> ServerMessage {
        self.state = accepted.state;
        self.outcome = accepted.outcome;
        self.seq += 1;
        accepted.applied
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
