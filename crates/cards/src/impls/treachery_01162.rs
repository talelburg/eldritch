//! Grasping Hands (The Gathering treachery, 01162).
//!
//! ```text
//! Revelation - Test [agility] (3). For each point you fail by, take 1 damage.
//! ```
//!
//! Pure DSL: `Trigger::Revelation` → `Effect::SkillTest` whose `on_fail`
//! deals 1 damage per point the test was failed by (#286 machinery).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    deal_damage, for_each_point_failed, revelation, skill_test, Ability, InvestigatorTarget,
};

/// `ArkhamDB` code for Grasping Hands.
pub const CODE: &str = "01162";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Agility,
        3,
        None,
        Some(for_each_point_failed(deal_damage(
            InvestigatorTarget::You,
            1,
        ))),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Effect;

    #[test]
    fn revelation_tests_agility_3_then_damage_per_point() {
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
        assert_eq!(*skill, SkillKind::Agility);
        assert_eq!(*difficulty, 3);
        assert!(on_success.is_none(), "no success-side effect");
        assert!(matches!(
            on_fail.as_deref(),
            Some(Effect::ForEachPointFailed(b))
                if matches!(**b, Effect::DealDamage { amount: 1, .. })
        ));
    }
}
