//! .41 Derringer (Rogue firearm asset, 01047).
//!
//! ```text
//! Uses (3 ammo).
//! [action] Spend 1 ammo: Fight. You get +2 [combat] for this attack. If you
//! succeed by 2 or more, this attack deals +1 damage.
//! ```
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! **Designator: Fight** — the bold word, which **performs the attack**
//! carrying the `+2 [combat]` and the conditional `+1` damage.
//!
//! The .45 Automatic 01016's shape with Switchblade 01044's damage clause.
//! Ammo (3) comes from the corpus (`CardKind::Asset.uses`, pipeline-parsed),
//! and the ability spends 1 per use via `Cost::SpendUses`. The margin is
//! [`Quantity::SkillTestSucceededBy`], bound when the Fight follow-up evaluates
//! `extra_damage` at RR ST.7, so it counts the `+2` and anything committed.
//!
//! [`Quantity::SkillTestSucceededBy`]: card_dsl::dsl::Quantity::SkillTestSucceededBy

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{self, Ability, Cost, IntExpr};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the .41 Derringer (original-Core printing).
pub const CODE: &str = "01047";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The .41 Derringer's one printed ability: *"\[action\] Spend 1 ammo:
/// **Fight.** You get +2 \[combat\] for this attack. If you succeed by 2 or
/// more, this attack deals +1 damage."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::fight(2u8, IntExpr::cond(dsl::succeeded_by(2), 1, 0)),
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        }],
        dsl::seq([]),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Effect, Trigger};

    use super::*;

    #[test]
    fn one_activated_fight_ability_spending_ammo() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Ammo,
                count: 1
            }]
        );
        let Some(ActionDesignator::Fight {
            combat_modifier, ..
        }) = &abilities[0].designator
        else {
            panic!(
                "expected a Fight designator, got {:?}",
                abilities[0].designator
            );
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(2));
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
