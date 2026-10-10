//! Grasping Hands (The Gathering treachery, 01162).
//!
//! ```text
//! Revelation - Test [agility] (3). For each point you fail by, take 1 damage.
//! ```
//!
//! Pure DSL: `Trigger::Revelation` → `Effect::SkillTest` whose `on_fail`
//! deals `Count(SkillTestFailedBy)` damage in a single `Deal` (#426).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{self, Ability, IntExpr, InvestigatorTarget, Quantity};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Grasping Hands.
pub const CODE: &str = "01162";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::skill_test(
        SkillKind::Agility,
        3,
        None,
        Some(dsl::deal_damage(
            InvestigatorTarget::You,
            IntExpr::Count(Quantity::SkillTestFailedBy),
        )),
    ))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, HarmKind, IntExpr, Quantity};

    use super::*;

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
            Some(Effect::Deal {
                kind: HarmKind::Damage,
                amount: IntExpr::Count(Quantity::SkillTestFailedBy),
                ..
            })
        ));
    }
}
