//! The minimum a scenario needs to exist.
//!
//! Teaching example — a Phase-7 implementer reading this should see
//! the shape of a scenario module without having to grok any real
//! scenario's content. One investigator, one location, a
//! one-line resolution predicate.

use std::collections::VecDeque;

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};

use super::synth_cards::{SYNTH_LOC_CODE, SYNTH_TREACHERY_CODE};

/// String id used to look this module up in
/// [`crate::REGISTRY`].
pub const ID: &str = "synthetic";

/// Build the initial [`GameState`] for this fixture: one
/// investigator, one location (with `code` set to
/// [`synth_cards::SYNTH_LOC_CODE`]), `scenario_id` set, `turn_order`
/// populated, encounter deck seeded with one copy of
/// [`synth_cards::SYNTH_TREACHERY_CODE`]. Phase = Mythos, round =
/// 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
///
/// The encounter-deck seeding gives the #126 / #127 integration
/// tests something to draw from; integration tests that want to
/// exercise spawn-bearing enemy reveals push the synthetic enemy
/// code (`synth_cards::SYNTH_ENEMY_CODE`) onto the deck themselves
/// after calling `setup()`.
///
/// [`synth_cards::SYNTH_LOC_CODE`]: super::synth_cards::SYNTH_LOC_CODE
/// [`synth_cards::SYNTH_TREACHERY_CODE`]: super::synth_cards::SYNTH_TREACHERY_CODE
/// [`synth_cards::SYNTH_ENEMY_CODE`]: super::synth_cards::SYNTH_ENEMY_CODE
pub fn setup() -> GameState {
    let mut location = test_location(10, "Demo Location");
    location.code = CardCode(SYNTH_LOC_CODE.into());

    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(location)
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_TREACHERY_CODE.into()));
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

/// Resolves with [`Resolution::Won`] once the engine has stepped
/// past `StartScenario`'s automatic Mythos skip into
/// [`Phase::Investigation`] with `round >= 1`.
///
/// One-liner deliberately: the integration test asserts this fires
/// after a single `StartScenario` apply.
#[must_use]
pub fn detect_resolution(state: &GameState) -> Option<Resolution> {
    if state.phase == Phase::Investigation && state.round >= 1 {
        Some(Resolution::Won { id: "demo".into() })
    } else {
        None
    }
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
/// the three `fn` pointers above; referenced from
/// [`crate::module_for`].
pub const MODULE: ScenarioModule = ScenarioModule {
    setup,
    detect_resolution,
    apply_resolution,
};
