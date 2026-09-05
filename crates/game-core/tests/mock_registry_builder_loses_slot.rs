//! [`MockRegistry::install`] against a slot a *foreign* registry already holds.
//!
//! The companion to `mock_registry_builder.rs`'s first-install-wins test, which
//! only covers two `MockRegistry`s racing each other. This one pins the harder
//! half: a builder that loses to something else — here `install_test_registry()`
//! — must leave nothing behind. If it stored its tables anyway, they would sit
//! there holding cards no installed lookup ever consults, and the binary that
//! registered them would fail with an unregistered-code symptom well away from
//! the cause.
//!
//! Its own binary because the assertion needs to own the process-global slot and
//! hand it to the *other* installer first.

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::card_registry;
use game_core::state::CardCode;
use game_core::test_support::{install_test_registry, MockRegistry, TEST_INV};

/// A probe card the losing builder registers, and nothing serves.
const PROBE: &str = "_mrbl_probe";

fn probe_metadata() -> CardMetadata {
    CardMetadata {
        code: PROBE.to_owned(),
        name: "Probe".to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

#[test]
fn a_builder_that_loses_the_slot_stores_nothing() {
    install_test_registry();
    MockRegistry::new().with_card(probe_metadata()).install();

    let registry = card_registry::current().expect("the standard test registry is installed");
    assert!(
        (registry.metadata_for)(&CardCode::new(PROBE)).is_none(),
        "the losing builder's card must not resolve through the registry that won"
    );
    assert!(
        (registry.metadata_for)(&CardCode::new(TEST_INV)).is_some(),
        "the winner still serves its own codes"
    );
}
