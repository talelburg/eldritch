//! Rotting Remains (The Gathering treachery, 01163).
//!
//! ```text
//! Revelation - Test [willpower] (3). For each point you fail by, take 1 horror.
//! ```
//!
//! Pure DSL: `Trigger::Revelation` → `Effect::SkillTest` whose `on_fail`
//! deals 1 horror per point the test was failed by (#286 machinery).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    deal_horror, for_each_point_failed, revelation, skill_test, Ability, InvestigatorTarget,
};

/// `ArkhamDB` code for Rotting Remains.
pub const CODE: &str = "01163";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Willpower,
        3,
        for_each_point_failed(deal_horror(InvestigatorTarget::You, 1)),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_willpower_3_then_horror_per_point() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } = &abilities[0].effect
        else {
            panic!("expected SkillTest, got {:?}", abilities[0].effect);
        };
        assert_eq!(*skill, SkillKind::Willpower);
        assert_eq!(*difficulty, 3);
        assert!(on_success.is_none(), "no success-side effect");
        assert!(matches!(
            **on_fail,
            Effect::ForEachPointFailed(ref b)
                if matches!(**b, Effect::DealHorror { amount: 1, .. })
        ));
    }
}
