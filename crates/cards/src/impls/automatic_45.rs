//! .45 Automatic (Guardian firearm asset, 01016).
//!
//! ```text
//! Uses (4 ammo).
//! [action] Spend 1 ammo: Fight. You get +1 [combat] for this attack.
//! This attack deals +1 damage.
//! ```
//!
//! The same shape as Roland's .38 Special (01006), only simpler: a flat
//! `+1` combat modifier instead of the clue-conditional `+1/+3`. Ammo (4)
//! comes from the corpus (`CardKind::Asset.uses`, pipeline-parsed); the
//! ability spends 1 per use via `Cost::SpendUses` and fights through the
//! inspectable `Effect::Fight`, dealing `1 + 1` damage on success.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated, fight, Ability, Cost, IntExpr};

/// `ArkhamDB` code for the .45 Automatic (original-Core printing).
pub const CODE: &str = "01016";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        }],
        fight(IntExpr::Lit(1), 1),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

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
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::Fight");
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(1));
        assert_eq!(*extra_damage, 1);
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
