//! The minimum a scenario needs to exist.
//!
//! Teaching example — a Phase-7 implementer reading this should see
//! the shape of a scenario module without having to grok any real
//! scenario's content. One investigator, one location, seeded
//! two-card act/agenda decks whose terminal cards carry resolution
//! points (push-model: the engine latches `GameState.resolution`
//! when an act/agenda resolution point is reached).

use std::collections::VecDeque;

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId,
};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};

use super::synth_cards::{SYNTH_LOC_CODE, SYNTH_TREACHERY_CODE};

/// String id used to look this module up in
/// [`crate::REGISTRY`].
pub const ID: &str = "synthetic";

/// Build the initial [`GameState`] for this fixture: one investigator
/// placed at one location (with `code` set to
/// [`synth_cards::SYNTH_LOC_CODE`], stocked with 4 clues), a +0 chaos
/// bag, `scenario_id` set, `turn_order` populated, encounter deck seeded
/// with one copy of [`synth_cards::SYNTH_TREACHERY_CODE`]. Phase =
/// Mythos, round = 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
///
/// The state is **playable to a resolution as-is** (no extra seeding):
/// placement + clues + a non-empty chaos bag are exactly what the
/// Investigate → discover-clue → `AdvanceAct` (Won) path needs, which is
/// what the browser demo and `tests/closing_demo.rs` exercise. A real
/// scenario's setup does the same (seats investigators, prints clue
/// counts on locations, fills the chaos bag); the bare `GameStateBuilder` /
/// `test_location` defaults (unplaced, 0 clues, empty bag) do not.
///
/// The encounter-deck seeding gives the #126 / #127 integration
/// tests something to draw from; integration tests that want to
/// exercise spawn-bearing enemy reveals push the synthetic enemy
/// code (`synth_cards::SYNTH_ENEMY_CODE`) onto the deck themselves
/// after calling `setup()`.
///
/// Also seeds two-card act and agenda decks. Each deck's first card
/// is non-terminal (`resolution: None`) and its second carries a
/// resolution point — advancing past the terminal card latches
/// `GameState.resolution` (act → `Won { id: "demo" }`, agenda →
/// `Lost { reason: "agenda" }`), driving the push-model hook.
///
/// [`synth_cards::SYNTH_LOC_CODE`]: super::synth_cards::SYNTH_LOC_CODE
/// [`synth_cards::SYNTH_TREACHERY_CODE`]: super::synth_cards::SYNTH_TREACHERY_CODE
/// [`synth_cards::SYNTH_ENEMY_CODE`]: super::synth_cards::SYNTH_ENEMY_CODE
pub fn setup() -> GameState {
    let mut location = test_location(10, "Demo Location");
    location.code = CardCode(SYNTH_LOC_CODE.into());
    // Stock the location with enough clues to advance both acts
    // (clue_threshold 2 each): four successful Investigates discover
    // four clues, exactly the Won path. Real scenarios print clue
    // counts on locations; the bare `test_location` default is 0.
    location.clues = 4;

    let mut state = GameStateBuilder::new()
        // Place the investigator at the demo location: scenario setup
        // seats investigators at a starting location, and Investigate /
        // Move reject on a `None` current_location. `test_location(10, …)`
        // → `LocationId(10)`.
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(location)
        .with_turn_order([InvestigatorId(1)])
        // A skill test rejects on an empty chaos bag; a single +0 token
        // lets Investigate (intellect 3) clear the location's shroud 2,
        // so the Won path is reliably walkable. Real scenarios fill the
        // bag from the campaign; this is the toy-fixture stand-in.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_scenario_id(ScenarioId::new(ID))
        .build();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_TREACHERY_CODE.into()));
    state.agenda_deck = vec![
        Agenda {
            code: CardCode("_synth_agenda_1".into()),
            doom_threshold: 2,
            resolution: None,
        },
        Agenda {
            code: CardCode("_synth_agenda_2".into()),
            doom_threshold: 2,
            resolution: Some(Resolution::Lost {
                reason: "agenda".into(),
            }),
        },
    ];
    state.act_deck = vec![
        Act {
            code: CardCode("_synth_act_1".into()),
            clue_threshold: 2,
            resolution: None,
        },
        Act {
            code: CardCode("_synth_act_2".into()),
            clue_threshold: 2,
            resolution: Some(Resolution::Won { id: "demo".into() }),
        },
    ];
    state
}

/// Seed the encounter deck of `state` with the given card codes,
/// in draw order (top of deck = index 0). Replaces whatever was
/// already in the deck.
///
/// Used by Phase-4 integration tests that want to drive the Mythos
/// phase through a deterministic card sequence without pushing codes
/// one-by-one onto the deck by hand.
pub fn with_encounter_deck(state: &mut GameState, codes: Vec<CardCode>) {
    state.encounter_deck = VecDeque::from(codes);
}

/// No-op. Phase 9 fills in real bodies once campaign-log XP / trauma
/// application lands.
pub fn apply_resolution(
    _resolution: &Resolution,
    _state: &mut GameState,
    _events: &mut Vec<Event>,
) {
}

/// The [`ScenarioModule`] value for the synthetic fixture. Bundles
/// the `setup` / `apply_resolution` `fn` pointers above; referenced
/// from [`crate::module_for`].
pub const MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup,
    apply_resolution,
};
