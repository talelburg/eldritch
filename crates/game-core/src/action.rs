//! Actions: the alphabet of the action log.
//!
//! Every change to game state happens by applying an [`Action`]. The
//! action log is a flat sequence of these, replayable into bit-identical
//! state. There are two kinds of actions, distinguished by who or what
//! initiated them:
//!
//! - [`Action::Player`] wraps a [`PlayerAction`] — input from a human via
//!   the websocket. Submitting a player action is the only thing a client
//!   can do; the wire protocol parses incoming messages as `PlayerAction`,
//!   so a client cannot fabricate engine-only events.
//! - [`Action::Engine`] wraps an [`EngineRecord`] — recorded output of
//!   engine-side randomness or system events (chaos token draws, deck
//!   shuffles). The engine generates these itself so the action log is
//!   replayable; clients never construct them.

use serde::{Deserialize, Serialize};

use crate::state::{ChaosToken, InvestigatorId, LocationId};

/// A single entry in the action log.
///
/// See the module docs for the rationale of the two-bucket split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Action initiated by a human player.
    Player(PlayerAction),
    /// Action emitted by the engine itself, recording a system event or
    /// random draw.
    Engine(EngineRecord),
}

/// Input from a human player.
///
/// Phase-1 minimal set; later phases add `Investigate`, `Move`, `Fight`,
/// `Evade`, `PlayCard`, `ActivateAbility`, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerAction {
    /// Begin a scenario session. Carries no payload at this stage; later
    /// phases will attach scenario code, investigator selections, deck
    /// snapshots, etc.
    StartScenario,
    /// Active investigator ends their turn during the Investigation phase.
    EndTurn,
    /// Respond to an `AwaitingInput` prompt the engine emitted. The
    /// shape of `response` is dictated by the active prompt. (The
    /// `EngineOutcome::AwaitingInput` variant lands in a later PR.)
    ResolveInput {
        /// The chosen response payload.
        response: InputResponse,
    },
}

/// Engine-recorded events.
///
/// Anything non-deterministic from the engine's perspective (RNG draws,
/// timer-derived values) goes into the log as one of these variants so
/// replay produces identical results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineRecord {
    /// A chaos token was drawn from the bag.
    ChaosTokenDrawn {
        /// The token that was drawn.
        token: ChaosToken,
    },
    /// A deck was shuffled with the given seed. Replays use the same seed
    /// to reproduce the order.
    DeckShuffled {
        /// Seed used for the shuffle.
        seed: u64,
    },
}

/// The shape of a response to an `AwaitingInput` prompt.
///
/// Phase-1 minimal set; later phases add target lists, card commits,
/// multi-target picks, etc. (The `EngineOutcome::AwaitingInput` variant
/// lands in a later PR.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputResponse {
    /// Confirm / yes / proceed with the prompted action.
    Confirm,
    /// Decline / skip the prompted optional action.
    Skip,
    /// Pick the option at the given zero-based index from the prompt's
    /// option list.
    PickIndex(usize),
    /// Pick a specific investigator.
    PickInvestigator(InvestigatorId),
    /// Pick a specific location.
    PickLocation(LocationId),
}
