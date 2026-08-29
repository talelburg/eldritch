//! Roland's .38 Special (Roland Banks signature asset, 01006).
//!
//! ```text
//! Roland Banks deck only.
//! Uses (4 ammo).
//! [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack
//! (if there are 1 or more clues on your location, you get +3 [combat],
//! instead). This attack deals +1 damage.
//! ```
//!
//! **Designator: Fight** — the bold word, which **performs the attack**
//! carrying the ability's combat modifier and its `+1` damage (#805), and which
//! is what exempts the activation from attacks of opportunity.
//!
//! Ammo (4) comes from the corpus (`CardKind::Asset.uses`, pipeline-
//! parsed); the ability spends 1 per use via `Cost::SpendUses`. The combat
//! modifier is `+3` when the investigator's location holds a clue and `+1`
//! otherwise (`IntExpr::cond` over
//! `Condition::Compare { CluesAtControllerLocation, Gt, 0 }`), and the attack
//! deals `1 + 1` damage on success.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated_as, fight, seq, Ability, CmpOp, Condition, Cost, IntExpr, Quantity};

/// `ArkhamDB` code for Roland's .38 Special.
pub const CODE: &str = "01006";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(
        fight(
            IntExpr::cond(
                Condition::Compare {
                    quantity: Quantity::CluesAtControllerLocation,
                    op: CmpOp::Gt,
                    value: 0,
                },
                3,
                1,
            ),
            1u8,
        ),
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        }],
        seq([]),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{ActionDesignator, Effect, Trigger};

    #[test]
    fn one_activated_fight_ability_spending_ammo() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let Trigger::Activated {
            action_cost: 1,
            designator:
                Some(ActionDesignator::Fight {
                    combat_modifier,
                    extra_damage,
                }),
        } = &abilities[0].trigger
        else {
            panic!(
                "expected a 1-action Fight designator, got {:?}",
                abilities[0].trigger
            );
        };
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Ammo,
                count: 1
            }]
        );
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
        assert_eq!(*extra_damage, IntExpr::Lit(1));
        assert_eq!(
            *combat_modifier,
            IntExpr::cond(
                Condition::Compare {
                    quantity: Quantity::CluesAtControllerLocation,
                    op: CmpOp::Gt,
                    value: 0,
                },
                3,
                1,
            )
        );
    }
}
