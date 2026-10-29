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
//! - [`Action::Engine`] wraps an [`EngineRecord`] — a system event
//!   (a deck shuffle, an encounter reveal) applied without a player
//!   decision. Production code never constructs one; only tests issue
//!   them, to drive those handlers directly. Replay stays deterministic
//!   because every random draw comes from the seeded `RngState` in
//!   `GameState`, not because draws are logged.

use serde::{Deserialize, Serialize};

use crate::engine::OptionId;
use crate::state::{CardCode, InvestigatorId};

/// A single entry in the action log.
///
/// See the module docs for the rationale of the two-bucket split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Action {
    /// Action initiated by a human player.
    Player(PlayerAction),
    /// A system event applied without a player decision. Only tests
    /// issue one.
    Engine(EngineRecord),
}

/// Input from a human player.
///
/// The action log is `ResolveInput`-only. Session setup
/// (seating investigators, starting the scenario) is handled by the
/// non-logged `seat_and_open` entry point and never appears here.
/// Open-turn gameplay flows through `ResolveInput(PickSingle(OptionId))`
/// against the `InvestigatorTurn` menu, dispatched internally via the
/// `TurnAction` id→action map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlayerAction {
    /// Respond to an [`AwaitingInput`](crate::engine::EngineOutcome::AwaitingInput)
    /// prompt the engine emitted. The shape of `response` is dictated by the
    /// active prompt — the open-turn action menu and every framework suspension
    /// (mulligan, encounter draw, skill-test commit, reaction/Fast windows,
    /// choices, soak distribution) all round-trip through this one channel.
    ResolveInput {
        /// The chosen response payload.
        response: InputResponse,
    },
}

/// One seat in a scenario: which investigator, the deck the player chose for
/// them, and how many random basic weaknesses to shuffle into it.
///
/// Crosses the wire to the host, which seats it through
/// `engine::seat_and_open`. Seating is not a logged action: the seated state is
/// frozen as the game's seed state, so the deck composition, drawn weaknesses
/// included, replays deterministically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// Investigator card code (e.g. `"01001"` for Roland Banks).
    pub investigator: CardCode,
    /// The player's chosen deck, top-to-bottom. Seating refuses it if it
    /// holds an unimplemented card, then appends the drawn random basic
    /// weaknesses before the setup shuffle. It does not check deckbuilding
    /// legality (TODO(#783): a deckbuilding validator). A **fixed** weakness,
    /// such as one carried between
    /// scenarios of a campaign, is simply a card here.
    pub deck: Vec<CardCode>,
    /// How many random basic weaknesses seating draws from the weakness pool
    /// and adds to [`deck`](Self::deck). Every investigator's card back asks
    /// for one; `0` asks for none. Draws share one pool across the roster and
    /// are without replacement, so a roster asking for more than the pool
    /// holds is refused.
    pub random_basic_weaknesses: u8,
}

/// Engine-recorded events.
///
/// Anything that doesn't originate from a single player action but
/// still needs an action-log entry for replay clarity. Chaos token
/// draws and inline-during-handler deck shuffles do NOT use this
/// channel — they happen as side effects of the action that triggered
/// them (e.g. `seat_and_open` shuffles every player deck during scenario
/// setup), and RNG determinism reproduces them from the same triggering
/// action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineRecord {
    /// Shuffle the named investigator's player deck.
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
    /// deck — the engine-record draw, separate from the Mythos draw loop's
    /// frames. Tests issue it directly to exercise the on-draw path.
    ///
    /// The reveal flow: the dispatch handler (`encounter_card_revealed`
    /// in `engine::dispatch`) draws the top of the deck (transparently
    /// reshuffling discard if needed), emits `Event::CardRevealed`, runs
    /// any `Trigger::Revelation` abilities through the DSL evaluator,
    /// then routes by card type (treachery → discard; enemy → spawn
    /// handler).
    EncounterCardRevealed {
        /// The investigator whose draw produced this reveal.
        investigator: InvestigatorId,
    },
}

/// The shape of a response to an `AwaitingInput` prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InputResponse {
    /// Confirm / yes / proceed with the prompted action.
    Confirm,
    /// Decline / skip the prompted optional action.
    Skip,
    /// Pick one option from a structured choice prompt
    /// ([`InputRequest::pick_single`](crate::engine::InputRequest::pick_single)),
    /// echoing back its [`OptionId`]. The
    /// single-selection family: the effect-choice machinery, and
    /// the location/investigator-pick windows (hunter move/engage, spawn engage)
    /// whose offered options index the candidate list.
    PickSingle(OptionId),
    /// Select a subset of the offered options, echoing back their
    /// [`OptionId`]s
    /// ([`InputRequest::pick_multiple`](crate::engine::InputRequest::pick_multiple)).
    /// The multi-selection family: the skill-test commit window, the setup
    /// mulligan and the upkeep hand-size discard. How many may be selected is
    /// the [`PickCount`](crate::engine::PickCount) its
    /// [`kind`](crate::engine::InputKind::PickMultiple) carries.
    PickMultiple {
        /// The chosen option ids, each naming one of the request's options.
        selected: Vec<OptionId>,
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

// The wire response type is these tests' subject, so they build
// `InputResponse` and `OptionId` values by hand rather than through a session.
#[cfg(test)]
mod input_response_tests {
    use super::*;

    #[test]
    fn pick_multiple_input_serde_roundtrip() {
        let original = InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(3), OptionId(7)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn pick_single_input_serde_roundtrip() {
        let original = InputResponse::PickSingle(OptionId(2));
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
