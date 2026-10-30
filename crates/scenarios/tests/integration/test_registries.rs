//! [`scenarios::TEST_REGISTRIES`]: the registries a scenario test names — the
//! real corpus with the synthetic test cards, and this crate's scenario
//! modules. This file installs nothing.

use game_core::scenario::ScenarioId;
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, TEST_INV};
use scenarios::{the_gathering, TEST_REGISTRIES};

#[test]
fn the_shipped_scenario_modules_are_served() {
    let module = (TEST_REGISTRIES.scenarios.module_for)(&ScenarioId::new(the_gathering::ID));
    assert!(module.is_some(), "The Gathering resolves");
}

#[test]
fn real_and_synthetic_investigators_are_seated_side_by_side() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001"))
        .with_seat(seat(2).as_card(TEST_INV))
        .build();
    assert_eq!(state.investigators[&InvestigatorId(1)].name, "Roland Banks");
    assert_eq!(
        state.investigators[&InvestigatorId(2)].name,
        "Test Investigator"
    );
}
