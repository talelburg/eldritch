//! .45 Automatic (Guardian firearm asset, 01016).
//!
//! ```text
//! Uses (4 ammo).
//! [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack.
//! This attack deals +1 damage.
//! ```
//!
//! **Designator: Fight** — the bold word, which **performs the attack**
//! carrying the ability's `+1 [combat]` and `+1` damage (#805), and which is
//! what exempts the activation from attacks of opportunity.
//!
//! The same shape as Roland's .38 Special (01006), only simpler: a flat
//! `+1` combat modifier instead of the clue-conditional `+1/+3`. Ammo (4)
//! comes from the corpus (`CardKind::Asset.uses`, pipeline-parsed); the
//! ability spends 1 per use via `Cost::SpendUses` and deals `1 + 1` damage on
//! success.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated_as, fight, seq, Ability, Cost};

/// `ArkhamDB` code for the .45 Automatic (original-Core printing).
pub const CODE: &str = "01016";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(
        fight(1u8, 1u8),
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
    use card_dsl::dsl::{ActionDesignator, Effect, IntExpr, Trigger};

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
        assert_eq!(*combat_modifier, IntExpr::Lit(1));
        assert_eq!(*extra_damage, IntExpr::Lit(1));
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
