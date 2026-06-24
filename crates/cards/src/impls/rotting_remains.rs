//! Rotting Remains (The Gathering treachery, 01163).
//!
//! ```text
//! Revelation - Test [willpower] (3). For each point you fail by, take 1 horror.
//! ```
//!
//! Pure DSL: `Trigger::Revelation` → `Effect::SkillTest` whose `on_fail`
//! deals `Count(SkillTestFailedBy)` horror in a single `Deal` (#426).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    deal_horror, revelation, skill_test, Ability, IntExpr, InvestigatorTarget, Quantity,
};

/// `ArkhamDB` code for Rotting Remains.
pub const CODE: &str = "01163";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(skill_test(
        SkillKind::Willpower,
        3,
        None,
        Some(deal_horror(
            InvestigatorTarget::You,
            IntExpr::Count(Quantity::SkillTestFailedBy),
        )),
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, HarmKind, IntExpr, Quantity};

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
            on_fail.as_deref(),
            Some(Effect::Deal {
                kind: HarmKind::Horror,
                amount: IntExpr::Count(Quantity::SkillTestFailedBy),
                ..
            })
        ));
    }
}
