//! [`MockRegistry`] — the shared builder for a test binary's mock card
//! registry. Composition of the standard lookups, per-slot dispatch, and
//! first-install-wins.
//!
//! An integration binary rather than an in-crate `#[cfg(test)]` module because
//! the assertions are *about* the process-global install, and `game-core`'s own
//! unit-test process already has `install_test_registry()` occupying that slot
//! (`card_registry::tests::install_is_idempotent_and_current_reflects_installed_value`).
//! A second claimant there would starve whichever test lost the race.
//!
//! The probe cards below are local to this file, per ADR 0016 — the builder
//! ships no named ones.

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::card_registry;
use game_core::dsl::{constant, modify, Ability, ModifierScope, Stat};
use game_core::state::{CardCode, GameState};
use game_core::test_support::{terminal_code, MockRegistry, TEST_INV};
use game_core::{Cx, EngineOutcome, EvalContext};

/// A probe card the installed registry knows about.
const PROBE: &str = "_mrb_probe";
/// A probe card registered only by the losing second install.
const LATE: &str = "_mrb_late";
/// A probe location whose ability is printed on its reverse.
const BACK: &str = "_mrb_back";

const EFFECT_TAG: &str = "_mrb:effect";
const ELIGIBILITY_TAG: &str = "_mrb:eligibility";
const CONDITION_TAG: &str = "_mrb:condition";

fn probe_metadata(code: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: format!("Probe {code}"),
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

fn front_ability() -> Vec<Ability> {
    vec![constant(modify(
        Stat::Willpower,
        1,
        ModifierScope::WhileInPlay,
    ))]
}

fn back_ability() -> Vec<Ability> {
    vec![constant(modify(
        Stat::Intellect,
        2,
        ModifierScope::WhileInPlay,
    ))]
}

fn probe_effect(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 7;
    EngineOutcome::Done
}

fn probe_eligibility(_state: &GameState, _ctx: &EvalContext) -> bool {
    true
}

fn probe_condition(_state: &GameState, _ctx: &EvalContext) -> bool {
    false
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_card(probe_metadata(PROBE))
        .with_abilities(PROBE, front_ability)
        .with_back_abilities(BACK, back_ability)
        .with_native_effect(EFFECT_TAG, probe_effect)
        .with_native_eligibility(ELIGIBILITY_TAG, probe_eligibility)
        .with_native_condition(CONDITION_TAG, probe_condition)
        .install();
}

fn registry() -> &'static card_registry::CardRegistry {
    card_registry::current().expect("the ctor installed a registry")
}

#[test]
fn registered_card_metadata_resolves() {
    let meta = (registry().metadata_for)(&CardCode::new(PROBE)).expect("probe is registered");
    assert_eq!(meta.code, PROBE);
    assert_eq!(meta.name, format!("Probe {PROBE}"));
}

#[test]
fn unregistered_code_resolves_to_nothing() {
    let unknown = CardCode::new("_mrb_unknown");
    assert!((registry().metadata_for)(&unknown).is_none());
    assert!((registry().abilities_for)(&unknown).is_none());
    assert!((registry().back_abilities_for)(&unknown).is_none());
}

/// The builder composes `metadata_for_test_inv`, so a binary that uses
/// `test_investigator` gets capacity lookups served without registering
/// `TEST_INV` itself.
#[test]
fn install_composes_the_test_investigator_lookup() {
    let meta = (registry().metadata_for)(&CardCode::new(TEST_INV))
        .expect("TEST_INV is composed in by install");
    assert_eq!(meta.code, TEST_INV);
}

/// And it composes `abilities_for_terminal`, so an act/agenda deck ending in a
/// `terminal_code` card gets its reverse served.
#[test]
fn install_composes_the_terminal_card_abilities() {
    let abilities =
        (registry().abilities_for)(&terminal_code(3)).expect("terminal cards are composed in");
    assert_eq!(abilities.len(), 2, "act-advanced and agenda-advanced");
}

#[test]
fn front_and_back_abilities_are_separate_slots() {
    let probe = CardCode::new(PROBE);
    let back = CardCode::new(BACK);
    assert_eq!((registry().abilities_for)(&probe).map(|a| a.len()), Some(1));
    assert!(
        (registry().back_abilities_for)(&probe).is_none(),
        "a front-only probe must not leak into the reverse slot"
    );
    assert_eq!(
        (registry().back_abilities_for)(&back).map(|a| a.len()),
        Some(1)
    );
    assert!(
        (registry().abilities_for)(&back).is_none(),
        "a reverse-only probe must not leak into the front slot"
    );
}

#[test]
fn native_tags_dispatch_per_slot() {
    assert!((registry().native_effect_for)(EFFECT_TAG).is_some());
    assert!((registry().native_eligibility_for)(EFFECT_TAG).is_none());
    assert!((registry().native_eligibility_for)(ELIGIBILITY_TAG).is_some());
    assert!((registry().native_condition_for)(CONDITION_TAG).is_some());
    assert!((registry().native_effect_for)("_mrb:missing").is_none());
}

/// First install wins: a second `install()` is a silent no-op, and neither the
/// registry it built nor its tables displace the first.
#[test]
fn install_is_idempotent_and_first_install_wins() {
    MockRegistry::new()
        .with_card(probe_metadata(LATE))
        .with_abilities(LATE, front_ability)
        .install();

    let late = CardCode::new(LATE);
    assert!(
        (registry().metadata_for)(&late).is_none(),
        "the second install must not register its cards"
    );
    assert!((registry().abilities_for)(&late).is_none());
    assert!(
        (registry().metadata_for)(&CardCode::new(PROBE)).is_some(),
        "the first install's cards survive"
    );
}
