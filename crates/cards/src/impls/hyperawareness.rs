//! Hyperawareness (Seeker asset, 01034).
//!
//! ```text
//! Talent.
//! [fast] Spend 1 resource: You get +1 [intellect] for this skill test.
//! [fast] Spend 1 resource: You get +1 [agility] for this skill test.
//! ```
//!
//! Two `[fast]` activated abilities, each paying 1 resource for a
//! `+1` push to the corresponding stat with
//! [`ModifierScope::ThisSkillTest`] scope. The DSL primitives that
//! make this expressible:
//!
//! - `Trigger::Activated { action_cost: 0 }` (#53) — `[fast]` means no
//!   action cost.
//! - `Cost::Resources(1)` (#53) — the per-ability payment.
//! - `ModifierScope::ThisSkillTest` evaluator push path (#102) —
//!   queues the modifier into [`GameState::pending_skill_modifiers`]
//!   for the next skill-test resolution to consume.
//!
//! [`GameState::pending_skill_modifiers`]: game_core::state::GameState::pending_skill_modifiers
//!
//! # Two abilities, two indices
//!
//! The intellect ability is at `ability_index: 0`; the agility
//! ability is at `ability_index: 1`. The order is significant — the
//! [`PlayerAction::ActivateAbility`] action carries the index, so
//! tests and clients must pick the matching slot.
//!
//! [`PlayerAction::ActivateAbility`]: game_core::PlayerAction::ActivateAbility

use game_core::dsl::{activated, modify, Ability, Cost, ModifierScope, Stat};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01034";

/// Hyperawareness's two activated `[fast]` abilities.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 intellect for this skill test.
        activated(
            0,
            vec![Cost::Resources(1)],
            modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
        ),
        // Index 1: +1 agility for this skill test.
        activated(
            0,
            vec![Cost::Resources(1)],
            modify(Stat::Agility, 1, ModifierScope::ThisSkillTest),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use game_core::dsl::{Cost, Effect, ModifierScope, Stat, Trigger};

    #[test]
    fn abilities_are_two_fast_activated_resource_costed_modifies() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        for (idx, ability) in abilities.iter().enumerate() {
            assert_eq!(
                ability.trigger,
                Trigger::Activated { action_cost: 0 },
                "ability {idx} must be [fast] (action_cost = 0)",
            );
            assert_eq!(
                ability.costs,
                vec![Cost::Resources(1)],
                "ability {idx} must cost exactly 1 resource",
            );
        }
    }

    #[test]
    fn intellect_ability_pushes_intellect_for_this_skill_test() {
        let intellect = &super::abilities()[0];
        assert!(matches!(
            intellect.effect,
            Effect::Modify {
                stat: Stat::Intellect,
                delta: 1,
                scope: ModifierScope::ThisSkillTest,
            }
        ));
    }

    #[test]
    fn agility_ability_pushes_agility_for_this_skill_test() {
        let agility = &super::abilities()[1];
        assert!(matches!(
            agility.effect,
            Effect::Modify {
                stat: Stat::Agility,
                delta: 1,
                scope: ModifierScope::ThisSkillTest,
            }
        ));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
