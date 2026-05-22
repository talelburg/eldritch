//! The minimum a scenario needs to exist.
//!
//! Teaching example — a Phase-7 implementer reading this should see
//! the shape of a scenario module without having to grok any real
//! scenario's content. One investigator, one location, a
//! one-line resolution predicate.

use game_core::event::Event;
use game_core::scenario::{Resolution, ScenarioId, ScenarioModule};
use game_core::state::{GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, test_location, TestGame};

/// String id used to look this module up in
/// [`crate::REGISTRY`].
pub const ID: &str = "synthetic";

/// Build the initial [`GameState`] for this fixture: one
/// investigator, one location, `scenario_id` set, `turn_order`
/// populated. Phase = Mythos, round = 0 — ready for
/// [`PlayerAction::StartScenario`](game_core::PlayerAction::StartScenario).
pub fn setup() -> GameState {
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(10, "Demo Location"))
        .with_turn_order([InvestigatorId(1)])
        .with_scenario_id(ScenarioId::new(ID))
        .build()
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
