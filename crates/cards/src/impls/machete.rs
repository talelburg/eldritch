//! Machete (Guardian melee weapon asset, 01020).
//!
//! ```text
//! [action]: Fight. You get +1 [combat] for this attack. If the attacked
//! enemy is the only enemy engaged with you, this attack deals +1 damage.
//! ```
//!
//! A bare `[action]` Fight (no exhaust, no uses) with a flat `+1` combat
//! modifier, dealing `1 + 1` damage on success.
//!
//! # The conditional `+1` damage is modeled as unconditional — on purpose
//!
//! Machete's bonus damage is printed as conditional on the attacked enemy
//! being "the only enemy engaged with you." We encode it as an
//! **unconditional** `extra_damage: 1`. This is exact under today's engine,
//! not an approximation: [`Effect::Fight`](card_dsl::dsl::Effect::Fight)
//! auto-targets the single engaged enemy, and `effect_initiates_fight`
//! rejects a fight with ≠1 engaged enemy **before** any cost is paid — so
//! at the moment Machete resolves, the attacked enemy is *necessarily* the
//! only enemy engaged with the controller, and the condition always holds.
//!
//! TODO(#300): when multi-target Fight lands (the #212/#213 cluster, where
//! the player chooses which engaged enemy to attack), this stops being
//! equivalent — Machete must then grant `+1` only when the attacked enemy
//! is the sole engaged one. At that point `Effect::Fight.extra_damage`
//! needs to become conditional (an `IntExpr` gated on a "sole engaged
//! enemy" `Condition`) and this impl updated.

use card_dsl::dsl::{activated, fight, Ability, IntExpr};

/// `ArkhamDB` code for Machete (original-Core printing).
pub const CODE: &str = "01020";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(1, vec![], fight(IntExpr::Lit(1), 1))]
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
        // Unconditional +1 — see the module doc-comment / TODO(#300).
        assert_eq!(*extra_damage, 1);
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
