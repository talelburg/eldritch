//! [`MockRegistry::build`]: a mock card registry as a value a test names,
//! Everything the fixtures resolve comes from the built value.
//!
//! The probe card below is local to this file, per ADR 0016.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, Skills};
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, MockRegistry, TEST_INV};

/// An investigator card only the built mock knows.
const PROBE_INV: &str = "_mrv_inv";

const PROBE_SKILLS: Skills = Skills {
    willpower: 1,
    intellect: 6,
    combat: 2,
    agility: 3,
};

/// The binary's one mock, built once on first use.
fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(CardMetadata {
                code: PROBE_INV.to_owned(),
                name: "Value Probe".to_owned(),
                traits: vec![],
                text: None,
                back_name: None,
                back_text: None,
                pack_code: "_mock".to_owned(),
                weakness: None,
                kind: CardKind::Investigator {
                    class: Class::Neutral,
                    skills: PROBE_SKILLS,
                    health: 7,
                    sanity: 7,
                },
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn a_built_mock_serves_its_cards_without_being_installed() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card(PROBE_INV))
        .build();
    assert_eq!(state.investigators[&InvestigatorId(1)].skills, PROBE_SKILLS);
}

#[test]
fn a_built_mock_composes_the_standard_test_investigator() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card(TEST_INV))
        .build();
    assert_eq!(
        state.investigators[&InvestigatorId(1)].name,
        "Test Investigator"
    );
}

#[test]
#[should_panic(expected = "already built")]
fn a_second_mock_in_one_process_panics() {
    let _ = registries();
    let _ = MockRegistry::new().build();
}
