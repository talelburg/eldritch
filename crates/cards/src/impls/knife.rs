//! Knife (neutral melee weapon asset, 01086).
//!
//! ```text
//! [action]: Fight. You get +1 [combat] for this attack.
//! [action] Discard Knife: Fight. You get +2 [combat] for this attack.
//!   This attack deals +1 damage.
//! ```
//!
//! **Designator: Fight**, on both abilities — the bold word that **performs**
//! each attack carrying that ability's modification (#805), and which is what
//! exempts either activation from attacks of opportunity.
//!
//! Two `[action]` Fight abilities, both pure compositions of existing
//! primitives:
//!
//! - **Ability 0** — a bare `[action]` Fight (no cost) with a `+1` combat
//!   modifier, dealing the base `1` damage (`extra_damage: 0`). The same
//!   shape as [`machete`](super::machete) minus the bonus damage.
//! - **Ability 1** — discards Knife itself ([`Cost::DiscardSelf`], #301) for
//!   a `+2` combat modifier and `+1` damage, dealing `1 + 1` on success.
//!
//! Both target a co-located enemy (choice-resolved when several are present
//! — #449/#451 widened the scope beyond engaged-only) and reject when no
//! eligible target exists **before** any cost is paid — so ability 1's
//! discard is never spent for nothing. The two abilities are at vec indices 0 and 1;
//! `ActivateAbility.ability_index` selects between them
//! (`resolve_activated_ability` indexes the raw abilities vec and rejects
//! any non-`Activated` trigger).

use card_dsl::dsl::{activated_as, fight, seq, Ability, Cost};

/// `ArkhamDB` code for Knife (original-Core printing).
pub const CODE: &str = "01086";

/// Knife's two `[action]` Fight abilities: the basic `+1 [combat]` fight and
/// the discard-self `+2 [combat]` / `+1` damage fight.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // [action]: Fight. You get +1 [combat] for this attack.
        activated_as(fight(1u8, 0u8), 1, vec![], seq([])),
        // [action] Discard Knife: Fight. You get +2 [combat] for this attack.
        // This attack deals +1 damage.
        activated_as(fight(2u8, 1u8), 1, vec![Cost::DiscardSelf], seq([])),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{ActionDesignator, Effect, IntExpr, Trigger};

    /// The `(combat_modifier, extra_damage)` the ability at `index` fights
    /// with, asserting on the way that it is a 1-action **Fight** designator
    /// with nothing printed beside it.
    fn fight_modification(
        abilities: &[card_dsl::dsl::Ability],
        index: usize,
    ) -> (IntExpr, IntExpr) {
        let Trigger::Activated {
            action_cost: 1,
            designator:
                Some(ActionDesignator::Fight {
                    combat_modifier,
                    extra_damage,
                }),
        } = &abilities[index].trigger
        else {
            panic!(
                "expected a 1-action Fight designator at index {index}, got {:?}",
                abilities[index].trigger
            );
        };
        assert_eq!(abilities[index].effect, Effect::Seq(vec![]));
        (combat_modifier.clone(), extra_damage.clone())
    }

    #[test]
    fn two_action_fight_abilities() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 2);

        // Index 0: bare [action] Fight, +1 combat, base damage.
        assert!(
            abilities[0].costs.is_empty(),
            "the basic Fight has no cost beyond the action",
        );
        assert_eq!(
            fight_modification(&abilities, 0),
            (IntExpr::Lit(1), IntExpr::Lit(0))
        );

        // Index 1: [action] Discard Knife Fight, +2 combat, +1 damage.
        assert_eq!(abilities[1].costs, vec![Cost::DiscardSelf]);
        assert_eq!(
            fight_modification(&abilities, 1),
            (IntExpr::Lit(2), IntExpr::Lit(1))
        );
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
