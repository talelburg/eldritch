//! [`cards::TEST_REGISTRY`]: the real corpus with the synthetic test cards
//! composed in, as a value a test names rather than installs. This binary
//! installs nothing, so every lookup below comes from the named value.

use cards::{TEST_REGISTRIES, TEST_REGISTRY};
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, Skills};
use game_core::test_support::{seat, terminal_code, TEST_INV};

const INV: InvestigatorId = InvestigatorId(1);

#[test]
fn a_real_investigator_is_seated_from_the_corpus() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001"))
        .build();
    let roland = &state.investigators[&INV];
    // Roland Banks 01001 prints willpower 3, intellect 3, combat 4, agility 2
    // (`data/arkhamdb-snapshot/pack/core/core.json`).
    assert_eq!(roland.name, "Roland Banks");
    assert_eq!(
        roland.skills,
        Skills {
            willpower: 3,
            intellect: 3,
            combat: 4,
            agility: 2,
        },
    );
}

#[test]
fn the_synthetic_test_investigator_resolves_next_to_the_corpus() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card(TEST_INV))
        .build();
    assert_eq!(state.investigators[&INV].name, "Test Investigator");
}

#[test]
fn the_synthetic_terminal_cards_and_real_abilities_are_both_served() {
    assert!((TEST_REGISTRY.abilities_for)(&terminal_code(1)).is_some());
    // Roland Banks 01001's ability is implemented in `impls/roland_banks.rs`.
    assert!((TEST_REGISTRY.abilities_for)(&CardCode::new("01001")).is_some());
}
