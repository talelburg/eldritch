//! Knife (neutral melee weapon asset, 01086).
//!
//! ```text
//! [action]: Fight. You get +1 [combat] for this attack.
//! [action] Discard Knife: Fight. You get +2 [combat] for this attack.
//!   This attack deals +1 damage.
//! ```
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
//! Both fight through the inspectable [`Effect::Fight`](card_dsl::dsl::Effect::Fight),
//! which auto-targets the single engaged enemy and rejects a fight with ≠1
//! engaged enemy **before** any cost is paid — so ability 1's discard is
//! never spent for nothing. The two abilities are at vec indices 0 and 1;
//! `ActivateAbility.ability_index` selects between them
//! (`resolve_activated_ability` indexes the raw abilities vec and rejects
//! any non-`Activated` trigger).

use card_dsl::dsl::{activated, fight, Ability, Cost, IntExpr};

/// `ArkhamDB` code for Knife (original-Core printing).
pub const CODE: &str = "01086";

/// Knife's two `[action]` Fight abilities: the basic `+1 [combat]` fight and
/// the discard-self `+2 [combat]` / `+1` damage fight.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // [action]: Fight. You get +1 [combat] for this attack.
        activated(1, vec![], fight(IntExpr::Lit(1), 0)),
        // [action] Discard Knife: Fight. You get +2 [combat] for this attack.
        // This attack deals +1 damage.
        activated(1, vec![Cost::DiscardSelf], fight(IntExpr::Lit(2), 1)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn two_action_fight_abilities() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 2);

        // Index 0: bare [action] Fight, +1 combat, base damage.
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(
            abilities[0].costs.is_empty(),
            "the basic Fight has no cost beyond the action",
        );
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::Fight at index 0");
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(1));
        assert_eq!(*extra_damage, 0);

        // Index 1: [action] Discard Knife Fight, +2 combat, +1 damage.
        assert_eq!(abilities[1].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(abilities[1].costs, vec![Cost::DiscardSelf]);
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[1].effect
        else {
            panic!("expected Effect::Fight at index 1");
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(2));
        assert_eq!(*extra_damage, 1);
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
