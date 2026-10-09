use card_dsl::card_data::SkillKind;

use super::*;
use crate::state::GameStateBuilder;
use crate::test_support;

#[test]
fn resolve_prey_default_single_candidate_is_one() {
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let r = resolve_prey(&state, Prey::Default, &[InvestigatorId(1)]);
    assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
}

#[test]
fn resolve_prey_default_multiple_is_tie() {
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .build();
    let r = resolve_prey(
        &state,
        Prey::Default,
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}

#[test]
fn resolve_prey_empty_is_none() {
    let state = GameStateBuilder::new().build();
    let r = resolve_prey(&state, Prey::Default, &[]);
    assert!(matches!(r, PreyResolution::None));
}

#[test]
fn resolve_prey_highest_stat_picks_max() {
    let mut hi = test_support::test_investigator(1);
    hi.skills.combat = 5;
    let mut lo = test_support::test_investigator(2);
    lo.skills.combat = 2;
    let state = GameStateBuilder::new()
        .with_investigator(hi)
        .with_investigator(lo)
        .build();
    let r = resolve_prey(
        &state,
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
    let mut a = test_support::test_investigator(1);
    a.skills.combat = 4;
    let mut b = test_support::test_investigator(2);
    b.skills.combat = 4;
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .build();
    let r = resolve_prey(
        &state,
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
    // max_health() = 8 (TEST_INV registry, #448 cp2a).
    // hurt: accumulated_damage 6 → remaining 2.
    // healthy: accumulated_damage 0 → remaining 8. hurt is lowest.
    let mut hurt = test_support::test_investigator(1);
    hurt.investigator_card.accumulated_damage = 6;
    let healthy = test_support::test_investigator(2);
    let state = GameStateBuilder::new()
        .with_investigator(hurt)
        .with_investigator(healthy)
        .build();
    let r = resolve_prey(
        &state,
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
    // max_health() = 8 (TEST_INV registry, #448 cp2a).
    // a: accumulated_damage 3 → remaining 5.
    // b: accumulated_damage 3 → remaining 5. Tie.
    let mut a = test_support::test_investigator(1);
    a.investigator_card.accumulated_damage = 3;
    let mut b = test_support::test_investigator(2);
    b.investigator_card.accumulated_damage = 3;
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .build();
    let r = resolve_prey(
        &state,
        Prey::Ranked {
            direction: PreyDirection::Lowest,
            measure: PreyMeasure::RemainingHealth,
        },
        &[InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
}
