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

use crate::state::{CardCode, InvestigatorId};

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
/// Two wire variants after 2b (#447): session setup ([`StartScenario`](Self::StartScenario))
/// and the menu-input channel ([`ResolveInput`](Self::ResolveInput)). Open-turn
/// gameplay is no longer typed — the engine surfaces its legal-action
/// enumeration as an open-turn `AwaitingInput` menu, and every gameplay action
/// flows through `ResolveInput(PickSingle(OptionId))` against it, dispatched
/// internally via the `TurnAction` id→action map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlayerAction {
    /// Begin a scenario session, seating the chosen investigators.
    ///
    /// `roster` pairs each investigator card code with the deck the
    /// player chose for them. Stats are resolved from card data at
    /// seat time (not carried here); the deck is taken verbatim — a
    /// free input that Phase 9's decklist import will populate.
    /// An empty roster seats no one; `start_scenario` rejects unless at
    /// least one investigator ends up seated.
    StartScenario { roster: Vec<RosterEntry> },
    /// Respond to an [`AwaitingInput`](crate::EngineOutcome::AwaitingInput)
    /// prompt the engine emitted. The shape of `response` is dictated by the
    /// active prompt — the open-turn action menu and every framework suspension
    /// (mulligan, encounter draw, skill-test commit, reaction/Fast windows,
    /// choices, soak distribution) all round-trip through this one channel
    /// (umbrella §3, #393).
    ResolveInput {
        /// The chosen response payload.
        response: InputResponse,
    },
}

/// One seat in a scenario: which investigator, and the deck the player
/// chose for them. Crosses the wire and lands in the action log, so the
/// deck composition replays deterministically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// Investigator card code (e.g. `"01001"` for Roland Banks).
    pub investigator: CardCode,
    /// The player's chosen deck, top-to-bottom. Taken verbatim by
    /// seating; deckbuilding-legality validation is Phase 9.
    pub deck: Vec<CardCode>,
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
    /// Shuffle the shared encounter deck. Reserved for explicit
    /// shuffle effects ("shuffle X into the encounter deck") — the
    /// empty-deck reshuffle inside `draw_encounter_top` (in
    /// `engine::dispatch`) happens as an in-handler side effect and
    /// does NOT push this variant. No payload — the deck is shared.
    EncounterDeckShuffled,
    /// The named investigator reveals the top card of the encounter
    /// deck. Emitted by #69's Mythos draw loop when it lands; in
    /// #126's tests, issued directly to exercise the on-draw path.
    ///
    /// The reveal flow: the dispatch handler (`encounter_card_revealed`
    /// in `engine::dispatch`) draws the top of the deck (transparently
    /// reshuffling discard if needed), emits `Event::CardRevealed`, runs
    /// any `Trigger::Revelation` abilities through the DSL evaluator,
    /// then routes by card type (treachery → discard; enemy → spawn
    /// handler from #127).
    EncounterCardRevealed {
        /// The investigator whose draw produced this reveal.
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
    /// Pick one option from a structured choice prompt
    /// ([`InputRequest::choice`](crate::engine::InputRequest::choice)),
    /// echoing back its [`OptionId`](crate::engine::OptionId). The
    /// single-selection family (umbrella §3): the Axis-A choice machinery, and
    /// the location/investigator-pick windows (hunter move/engage, spawn engage)
    /// whose offered options index the candidate list.
    PickSingle(crate::engine::OptionId),
    /// Select a subset of the offered options, echoing back their
    /// [`OptionId`](crate::engine::OptionId)s (umbrella §3). The multi-selection
    /// family — the skill-test commit window and the upkeep hand-size discard
    /// fold into this; min/exact-count constraints live on the request/frame,
    /// not here. For those windows an `OptionId(i)` denotes hand index `i`.
    PickMultiple {
        /// The chosen option ids (hand indices, for commit/discard windows).
        selected: Vec<crate::engine::OptionId>,
    },
}

#[cfg(test)]
mod encounter_card_revealed_action_tests {
    use super::*;

    #[test]
    fn encounter_card_revealed_engine_record_serde_roundtrip() {
        let rec = EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let back: EngineRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, rec);
    }
}

#[cfg(test)]
mod input_response_tests {
    use super::*;

    #[test]
    fn pick_multiple_input_serde_roundtrip() {
        use crate::engine::OptionId;
        let original = InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(3), OptionId(7)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn pick_single_input_serde_roundtrip() {
        use crate::engine::OptionId;
        let original = InputResponse::PickSingle(OptionId(2));
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
