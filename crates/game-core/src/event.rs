//! Events: state-change records emitted as actions resolve.
//!
//! When the engine applies an [`Action`], it produces a sequence of
//! [`Event`] values describing what changed. Events flow back to clients
//! over the websocket (clients update their local view by replaying
//! them) and are the substrate that triggered card abilities listen to.
//!
//! Events are NOT the source of truth for state — that's the action log.
//! Events are derived from action application and are useful as a
//! denormalized "what happened" stream.
//!
//! [`Action`]: crate::Action

use serde::{Deserialize, Serialize};

use crate::state::{ChaosToken, InvestigatorId, LocationId, Phase, TokenResolution};

/// One state-change record emitted by the engine.
///
/// Phase-1 minimal set. Later phases add events for skill-test
/// commits, card plays, ability triggers, encounter draws, doom changes,
/// trauma, scenario resolution, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// A scenario session has begun.
    ScenarioStarted,
    /// A new phase began.
    PhaseStarted {
        /// The phase that just started.
        phase: Phase,
    },
    /// A phase ended.
    PhaseEnded {
        /// The phase that just ended.
        phase: Phase,
    },
    /// An investigator's turn ended (Investigation phase).
    TurnEnded {
        /// Whose turn it was.
        investigator: InvestigatorId,
    },
    /// An investigator's action point count changed.
    ActionsRemainingChanged {
        /// Whose action count changed.
        investigator: InvestigatorId,
        /// New count.
        new_count: u8,
    },
    /// An investigator moved between locations.
    InvestigatorMoved {
        /// Who moved.
        investigator: InvestigatorId,
        /// Origin location.
        from: LocationId,
        /// Destination location.
        to: LocationId,
    },
    /// A chaos token was revealed during a skill test.
    ChaosTokenRevealed {
        /// The token revealed.
        token: ChaosToken,
        /// How the token resolves against the scenario's modifier table:
        /// a numeric modifier, [`AutoFail`], or [`ElderSign`].
        ///
        /// [`AutoFail`]: TokenResolution::AutoFail
        /// [`ElderSign`]: TokenResolution::ElderSign
        resolution: TokenResolution,
    },
    /// One or more clues moved to an investigator.
    CluePlaced {
        /// Who received the clues.
        investigator: InvestigatorId,
        /// Number of clues placed.
        count: u8,
    },
    /// A location's clue count changed.
    LocationCluesChanged {
        /// The location.
        location: LocationId,
        /// New clue count.
        new_count: u8,
    },
    /// An investigator suffered physical damage.
    DamageTaken {
        /// Who was damaged.
        investigator: InvestigatorId,
        /// Amount of damage.
        amount: u8,
    },
    /// An investigator suffered horror.
    HorrorTaken {
        /// Who took horror.
        investigator: InvestigatorId,
        /// Amount of horror.
        amount: u8,
    },
    /// An investigator gained resources.
    ResourcesGained {
        /// Who received resources.
        investigator: InvestigatorId,
        /// Amount gained.
        amount: u8,
    },
    /// An investigator paid / lost resources (e.g. as a `Cost::Resources`
    /// payment for an activated ability).
    ResourcesPaid {
        /// Who paid resources.
        investigator: InvestigatorId,
        /// Amount paid.
        amount: u8,
    },
}
