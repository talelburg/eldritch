//! Switchblade (Rogue melee weapon asset, 01044).
//!
//! ```text
//! Fast.
//! [action]: Fight. If you succeed by 2 or more, this attack deals +1 damage.
//! ```
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! **Designator: Fight** — the bold word, which **performs the attack** and
//! carries its conditional `+1` damage (#805). There is no combat modifier.
//!
//! The margin is [`Quantity::SkillTestSucceededBy`], which the Fight follow-up
//! binds when it evaluates `extra_damage` at RR ST.7, the moment the attack's
//! damage is determined. A card committed to the test after the ability was
//! activated therefore counts towards it.
//!
//! *"Fast."* is printed metadata (`is_fast`), which the play gate reads; it is
//! not an ability here.

use card_dsl::dsl::{self, Ability, CmpOp, Condition, IntExpr, Quantity};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Switchblade (original-Core printing).
pub const CODE: &str = "01044";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::fight(0u8, plus_one_if_succeeded_by_two()),
        1,
        vec![],
        dsl::seq([]),
    )]
}

/// *"If you succeed by 2 or more, this attack deals +1 damage."*
fn plus_one_if_succeeded_by_two() -> IntExpr {
    IntExpr::cond(
        Condition::Compare {
            quantity: Quantity::SkillTestSucceededBy,
            op: CmpOp::Ge,
            value: 2,
        },
        1,
        0,
    )
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Effect, Trigger};

    use super::*;

    #[test]
    fn one_costless_activated_fight_ability() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(abilities[0].costs.is_empty());
        let Some(ActionDesignator::Fight {
            combat_modifier,
            extra_damage,
            ..
        }) = &abilities[0].designator
        else {
            panic!(
                "expected a Fight designator, got {:?}",
                abilities[0].designator
            );
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(0));
        assert_eq!(*extra_damage, plus_one_if_succeeded_by_two());
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
