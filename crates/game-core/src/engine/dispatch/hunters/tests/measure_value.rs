use card_dsl::card_data::{CardMetadata, SkillKind};
use card_dsl::dsl::{self, Ability, ModifierScope, Stat};

use super::*;
use crate::card_registry::CardRegistry;
use crate::state::{CardCode, CardInPlay, CardInstanceId, GameStateBuilder};
use crate::test_support;

fn no_metadata(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn fake_abilities(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        "combat+1" => Some(vec![dsl::constant(dsl::modify(
            Stat::Combat,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        "combat-5" => Some(vec![dsl::constant(dsl::modify(
            Stat::Combat,
            -5,
            ModifierScope::WhileInPlay,
        ))]),
        "maxhealth+2" => Some(vec![dsl::constant(dsl::modify(
            Stat::MaxHealth,
            2,
            ModifierScope::WhileInPlay,
        ))]),
        _ => None,
    }
}

fn fake_registry() -> CardRegistry {
    CardRegistry {
        metadata_for: no_metadata,
        abilities_for: fake_abilities,
        ..CardRegistry::EMPTY
    }
}

/// Build a state holding investigator 1 (the modifier lookup reads
/// `state.investigators[inv.id].cards_in_play`, so the investigator
/// must be in the state, not merely passed by reference).
fn state_with(cards: &[&str], damage: u8) -> GameState {
    test_support::install_test_registry();
    let mut inv = test_support::test_investigator(1); // combat 3, max_health() = 8 from TEST_INV registry
                                                      // After #448 cp2a harm accumulates on the investigator card.
    inv.investigator_card.accumulated_damage = damage;
    inv.cards_in_play = cards
        .iter()
        .enumerate()
        .map(|(i, c)| {
            CardInPlay::enter_play(
                CardCode::new(*c),
                #[allow(clippy::cast_possible_truncation)]
                CardInstanceId(i as u32),
            )
        })
        .collect();
    GameStateBuilder::new().with_investigator(inv).build()
}

#[test]
fn base_value_when_no_registry() {
    let state = state_with(&[], 0);
    let inv = &state.investigators[&InvestigatorId(1)];
    assert_eq!(
        measure_value(&state, None, inv, PreyMeasure::Skill(SkillKind::Combat)),
        3
    );
}

#[test]
fn folds_constant_skill_modifier() {
    let state = state_with(&["combat+1"], 0);
    let inv = &state.investigators[&InvestigatorId(1)];
    let reg = fake_registry();
    assert_eq!(
        measure_value(
            &state,
            Some(&reg),
            inv,
            PreyMeasure::Skill(SkillKind::Combat)
        ),
        4
    );
}

#[test]
fn folds_max_health_modifier_into_remaining_health() {
    // max_health 8 − damage 1 + 2 = 9.
    let state = state_with(&["maxhealth+2"], 1);
    let inv = &state.investigators[&InvestigatorId(1)];
    let reg = fake_registry();
    assert_eq!(
        measure_value(&state, Some(&reg), inv, PreyMeasure::RemainingHealth),
        9
    );
}

#[test]
fn clamps_at_zero() {
    // combat 3 − 5 = −2, floored to 0 (RR p.15).
    let state = state_with(&["combat-5"], 0);
    let inv = &state.investigators[&InvestigatorId(1)];
    let reg = fake_registry();
    assert_eq!(
        measure_value(
            &state,
            Some(&reg),
            inv,
            PreyMeasure::Skill(SkillKind::Combat)
        ),
        0
    );
}
