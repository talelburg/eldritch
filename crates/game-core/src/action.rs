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

use crate::state::{EnemyId, InvestigatorId, LocationId, SkillKind};

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
    /// Engage an enemy with a combat skill test. Spends 1 action.
    /// On success, deals 1 damage to the enemy; if damage reaches
    /// `max_health`, the enemy is defeated and removed from play.
    ///
    /// Validate: Investigation phase, investigator is active,
    /// `actions_remaining >= 1`, enemy exists, and the investigator
    /// is currently engaged with that enemy.
    ///
    /// Damage > 1 (weapons, card buffs) and after-success / after-
    /// failure triggers (#64) land downstream. `AoO` does NOT fire on
    /// Fight (Fight is on the `AoO`-exempt list per the Rules
    /// Reference).
    Fight {
        /// Investigator performing the Fight action. Must be the
        /// active investigator during the Investigation phase.
        investigator: InvestigatorId,
        /// The enemy to fight. Must be currently engaged with the
        /// investigator.
        enemy: EnemyId,
    },
    /// Evade an engaged enemy with an agility skill test. Spends
    /// 1 action. On success, the enemy disengages and exhausts.
    ///
    /// Validate: Investigation phase, investigator is active,
    /// `actions_remaining >= 1`, enemy exists, and the investigator
    /// is currently engaged with that enemy.
    ///
    /// `AoO` does NOT fire on Evade (also on the `AoO`-exempt list).
    Evade {
        /// Investigator performing the Evade action. Must be the
        /// active investigator.
        investigator: InvestigatorId,
        /// The enemy to evade. Must be currently engaged with the
        /// investigator.
        enemy: EnemyId,
    },
    /// Redraw a subset of an investigator's starting hand. One-shot
    /// per investigator per scenario; only valid while the engine's
    /// mulligan window is open. The window opens at `StartScenario`
    /// and closes once every investigator has `mulligan_used == true`
    /// (per the Rules Reference: "after all players have completed
    /// their mulligans, the game begins").
    ///
    /// `indices_to_redraw` names zero-based positions in the hand to
    /// redraw. An empty vec is a legal "keep my hand" signal that
    /// still consumes the one-shot. Indices must be in bounds
    /// (`< hand.len()`) and unique.
    ///
    /// On apply: named cards move hand → deck directly (per the
    /// Rules Reference's "shuffles them back into his or her deck",
    /// NOT via the discard pile), the deck is shuffled, and the
    /// investigator draws replacement cards equal to the redraw
    /// count. An empty mulligan skips both the shuffle and the
    /// redraw — the deck stays untouched.
    Mulligan {
        /// Investigator mulliganing. Must be `Status::Active` and
        /// not have already used their mulligan this scenario.
        investigator: InvestigatorId,
        /// Hand indices to redraw; empty = no-op-but-consumes.
        indices_to_redraw: Vec<u8>,
    },
    /// Draw a card from the player deck. Standard turn-action:
    /// spends 1 action, draws 1 card.
    ///
    /// **Empty-deck reshuffle:** if the deck is empty and the
    /// discard is non-empty, shuffle the discard into the deck (via
    /// the shared RNG, emitting `DeckShuffled`), draw, then take
    /// 1 horror.
    ///
    /// **Both empty:** no shuffle, no card drawn, but the 1 horror
    /// still applies as the safer reading of "would-draw-from-empty-
    /// deck."
    Draw {
        /// Investigator drawing. Must be the active investigator
        /// during the Investigation phase, with status `Active`.
        investigator: InvestigatorId,
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
    /// Play a card from an investigator's hand. Spends no action
    /// point at this stage — play-cost gating (resource cost,
    /// action cost, "Fast" exemption) lives on the card and lands
    /// with the cost-primitive DSL work (#53).
    ///
    /// Validation: Investigation phase, investigator is active and
    /// `Status::Active`, `hand_index < hand.len()`, the registry is
    /// installed, the card code resolves to known metadata, and the
    /// card's type is one of the two hand-playable types — Asset or
    /// Event. Every other type rejects.
    ///
    /// On apply: emit [`CardPlayed`](crate::Event::CardPlayed), run
    /// every [`Trigger::OnPlay`](crate::dsl::Trigger::OnPlay) ability
    /// through the DSL evaluator, then move the card to its
    /// destination zone — `cards_in_play` for assets, `discard` for
    /// events (the latter emitting
    /// [`CardDiscarded`](crate::Event::CardDiscarded) with `from: Hand`).
    PlayCard {
        /// Investigator playing the card. Must be the active
        /// investigator during the Investigation phase, with status
        /// `Active`.
        investigator: InvestigatorId,
        /// Zero-based position in the investigator's hand.
        hand_index: u8,
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
/// Anything that doesn't originate from a single player action but
/// still needs an action-log entry for replay clarity. Chaos token
/// draws and inline-during-handler deck shuffles do NOT use this
/// channel — they happen as side effects of the action that
/// triggered them (e.g. [`PlayerAction::StartScenario`] shuffles
/// every player deck before the initial hand draw), and RNG
/// determinism reproduces them from the same triggering action.
///
/// The standalone [`DeckShuffled`](EngineRecord::DeckShuffled)
/// variant is for shuffle requests that don't come from a player
/// action — future card effects like "shuffle X into your deck" will
/// emit this when the effect resolves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineRecord {
    /// Shuffle the named investigator's player deck. Encounter deck
    /// and act/agenda decks add their own variants when they land.
    DeckShuffled {
        /// Whose deck to shuffle.
        investigator: InvestigatorId,
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
