use card_dsl::card_data::SkillKind;

use super::*;
use crate::state::GameStateBuilder;
use crate::test_support;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

#[test]
fn resolve_prey_default_single_candidate_is_one() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let r = resolve_prey(&state, &TEST_REGISTRY, Prey::Default, &[InvestigatorId(1)]);
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_default_multiple_is_tie() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .build();
    let r = resolve_prey(
        &state,
        &TEST_REGISTRY,
        Prey::Default,
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}

#[test]
fn resolve_prey_empty_is_none() {
    let state = GameStateBuilder::new().build();
    let r = resolve_prey(&state, &TEST_REGISTRY, Prey::Default, &[]);
    assert!(matches!(r, PreyResolution::None));
}

#[test]
fn resolve_prey_highest_stat_picks_max() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).skill(SkillKind::Combat, 5))
        .with_seat(test_support::seat(2).skill(SkillKind::Combat, 2))
        .build();
    let r = resolve_prey(
        &state,
        &TEST_REGISTRY,
        Prey::Ranked {
            direction: PreyDirection::Highest,
            measure: PreyMeasure::Skill(SkillKind::Combat),
        },
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_highest_stat_tie_is_tie() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).skill(SkillKind::Combat, 4))
        .with_seat(test_support::seat(2).skill(SkillKind::Combat, 4))
        .build();
    let r = resolve_prey(
        &state,
        &TEST_REGISTRY,
        Prey::Ranked {
            direction: PreyDirection::Highest,
            measure: PreyMeasure::Skill(SkillKind::Combat),
        },
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}

#[test]
fn resolve_prey_lowest_remaining_health_picks_min() {
    // max_health() = 8 (TEST_INV registry).
    // hurt: accumulated_damage 6 → remaining 2.
    // healthy: accumulated_damage 0 → remaining 8. hurt is lowest.
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).harm(6, 0))
        .with_seat(test_support::seat(2))
        .build();
    let r = resolve_prey(
        &state,
        &TEST_REGISTRY,
        Prey::Ranked {
            direction: PreyDirection::Lowest,
            measure: PreyMeasure::RemainingHealth,
        },
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_lowest_remaining_health_tie_is_tie() {
    // max_health() = 8 (TEST_INV registry).
    // a: accumulated_damage 3 → remaining 5.
    // b: accumulated_damage 3 → remaining 5. Tie.
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).harm(3, 0))
        .with_seat(test_support::seat(2).harm(3, 0))
        .build();
    let r = resolve_prey(
        &state,
        &TEST_REGISTRY,
        Prey::Ranked {
            direction: PreyDirection::Lowest,
            measure: PreyMeasure::RemainingHealth,
        },
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}
