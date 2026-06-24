//! Machete (Guardian melee weapon asset, 01020).
//!
//! ```text
//! [action]: Fight. You get +1 [combat] for this attack. If the attacked
//! enemy is the only enemy engaged with you, this attack deals +1 damage.
//! ```
//!
//! A bare `[action]` Fight (no exhaust, no uses) with a flat `+1` combat
//! modifier. The bonus damage is conditional: `+1` only when the attacked
//! enemy is the sole engaged enemy, encoded as
//! `IntExpr::cond(EngagedEnemies == 1, 1, 0)`.
//!
//! Note: activated `Effect::Fight` currently requires *exactly one* engaged
//! enemy (2+ engaged is rejected pre-cost, pending multi-target selection /
//! [#401](https://github.com/talelburg/eldritch/issues/401)). As a result the
//! `EngagedEnemies == 1` condition always holds whenever the Fight resolves
//! today — the `+0` branch is correct modelling but only becomes reachable
//! once 2-engaged activated Fight lands.

use card_dsl::dsl::{activated, fight, Ability, CmpOp, Condition, IntExpr, Quantity};

/// `ArkhamDB` code for Machete (original-Core printing).
pub const CODE: &str = "01020";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![],
        fight(
            1u8,
            IntExpr::cond(
                Condition::Compare {
                    quantity: Quantity::EngagedEnemies,
                    op: CmpOp::Eq,
                    value: 1,
                },
                1,
                0,
            ),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_costless_activated_fight_ability() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(
            abilities[0].costs.is_empty(),
            "Machete's Fight has no exhaust/uses cost — just the action",
        );
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::Fight");
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(1));
        // +1 only when the attacked enemy is the sole engaged enemy (#300).
        assert_eq!(
            *extra_damage,
            IntExpr::cond(
                Condition::Compare {
                    quantity: Quantity::EngagedEnemies,
                    op: CmpOp::Eq,
                    value: 1,
                },
                1,
                0,
            )
        );
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
