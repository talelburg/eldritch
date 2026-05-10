//! Actions: the alphabet of the action log.
//!
//! Every change to game state happens by applying an [`Action`]. The
//! action log is a flat sequence of these, replayable into bit-identical
//! state. There are two kinds of actions, distinguished by who or what
//! initiated them:
//!
//! - [`Action::Player`] wraps a [`PlayerAction`] — input crossing the
//!   transport boundary from a client. The wire layer parses incoming
//!   messages as `PlayerAction`, so a client cannot fabricate
//!   engine-only events.
//! - [`Action::Engine`] wraps an [`EngineRecord`] — recorded output of
//!   engine-side randomness or system events (deck shuffles). The
//!   engine generates these itself so the action log is replayable;
//!   clients never construct them.

use serde::{Deserialize, Serialize};

use crate::state::{InvestigatorId, LocationId, SkillKind};

/// A single entry in the action log.
///
/// See the module docs for the rationale of the two-bucket split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum PlayerAction {
    /// Begin a scenario session. Carries no payload at this stage; later
    /// phases will attach scenario code, investigator selections, deck
    /// snapshots, etc.
    StartScenario,
    /// Active investigator ends their turn during the Investigation phase.
    EndTurn,
    /// Perform a skill test on `investigator` against `difficulty`,
    /// using the named `skill`.
    ///
    /// Phase-1 foundation: resolves in one apply call as base skill +
    /// chaos token modifier vs. difficulty. Card commits, the commit
    /// window's `AwaitingInput`, and the after-resolution trigger
    /// window land in #63 / #64.
    PerformSkillTest {
        /// Investigator taking the test.
        investigator: InvestigatorId,
        /// Which of the four skills the test is against.
        skill: SkillKind,
        /// Difficulty: total to meet or exceed for success.
        difficulty: i8,
    },
    /// Move the active investigator from their current location to a
    /// connected location. Spends 1 action.
    ///
    /// Validation: investigation phase, investigator is active, has
    /// `actions_remaining >= 1`, has a `current_location`, and the
    /// destination is in `state.locations` and connected to the current
    /// location.
    ///
    /// Move *is* legal while engaged with enemies — but each ready
    /// engaged enemy makes an attack of opportunity before the move
    /// resolves, and engaged enemies move with the investigator.
    /// Both behaviors land alongside enemy state in #67; this handler
    /// covers only the bare movement.
    Move {
        /// Investigator performing the move. Must be the active
        /// investigator during the Investigation phase.
        investigator: InvestigatorId,
        /// The destination location. Must be in `state.locations` and
        /// connected to the investigator's current location.
        destination: LocationId,
    },
    /// Investigate at the active investigator's current location:
    /// spend 1 action, make an intellect test against the location's
    /// shroud value, and on success discover 1 clue at the location.
    ///
    /// Card-derived investigate variants (Rite of Seeking's "Action.
    /// Spend 1 charge: Investigate using willpower instead of intellect",
    /// Working a Hunch's discover-without-test) are the cards' concern,
    /// not this action.
    Investigate {
        /// Investigator performing the action. Must be the active
        /// investigator during the Investigation phase.
        investigator: InvestigatorId,
    },
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
/// Anything non-deterministic from the engine's perspective (timer-
/// derived values, deck shuffles) goes into the log as one of these
/// variants so replay produces identical results. Chaos token draws
/// are NOT recorded here — they happen inline as part of the action
/// that triggered them (e.g. `PerformSkillTest`); RNG determinism plus
/// the per-draw `Event::ChaosTokenRevealed` give replay equivalence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineRecord {
    /// A deck was shuffled with the given seed. Replays use the same seed
    /// to reproduce the order.
    //
    // TODO(#62): once there are multiple decks (encounter deck, each
    // investigator's deck, act/agenda decks), this needs a `deck:
    // DeckId` field to disambiguate.
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
#[non_exhaustive]
pub enum InputResponse {
    /// Confirm / yes / proceed with the prompted action.
    Confirm,
    /// Decline / skip the prompted optional action.
    Skip,
    /// Pick the option at the given zero-based index from the prompt's
    /// option list. `u32` rather than `usize` for a stable wire format
    /// independent of host pointer width.
    PickIndex(u32),
    /// Pick a specific investigator.
    PickInvestigator(InvestigatorId),
    /// Pick a specific location.
    PickLocation(LocationId),
}
