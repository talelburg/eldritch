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
//! - `Trigger::Activated { action_cost: 0 }` with no action designator (#53)
//!   — `[fast]` means no action cost.
//! - `Cost::Resources(1)` (#53) — the per-ability payment.
//! - `ModifierScope::ThisSkillTest` evaluator push path (#102) —
//!   records the modifier into [`GameState::recorded_modifiers`]
//!   for the next skill-test resolution to consume.
//!
//! [`GameState::recorded_modifiers`]: game_core::state::GameState::recorded_modifiers
//!
//! # Two abilities, two indices
//!
//! The intellect ability is at `ability_index: 0`; the agility
//! ability is at `ability_index: 1`. The order is significant — the
//! [`TurnAction::ActivateAbility`] action carries the index, so
//! tests and clients must pick the matching slot.
//!
//! [`TurnAction::ActivateAbility`]: game_core::engine::enumerate::TurnAction::ActivateAbility

use card_dsl::dsl::{self, Ability, Cost, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01034";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Hyperawareness's two activated `[fast]` abilities.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 intellect for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
        ),
        // Index 1: +1 agility for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Agility, 1, ModifierScope::ThisSkillTest),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Cost, Effect, ModifierAudience, ModifierScope, Stat, Trigger};

    #[test]
    fn abilities_are_two_fast_activated_resource_costed_modifies() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        for (idx, ability) in abilities.iter().enumerate() {
            assert_eq!(
                ability.trigger,
                Trigger::Activated {
                    action_cost: 0,
                    designator: None
                },
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
                audience: ModifierAudience::Controller,
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
                audience: ModifierAudience::Controller,
            }
        ));
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
