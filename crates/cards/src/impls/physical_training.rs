//! Physical Training (Guardian talent asset, 01017).
//!
//! ```text
//! Talent.
//! [fast] Spend 1 resource: You get +1 [willpower] for this skill test.
//! [fast] Spend 1 resource: You get +1 [combat] for this skill test.
//! ```
//!
//! Structurally identical to Hyperawareness (01034) — two `[fast]`
//! activated abilities, each paying 1 resource for a `+1` push to a stat
//! with [`ModifierScope::ThisSkillTest`] scope — only the stats differ
//! (willpower / combat here, intellect / agility there).
//!
//! # Two abilities, two indices
//!
//! The willpower ability is at `ability_index: 0`; the combat ability is
//! at `ability_index: 1`. The order is significant — the
//! [`PlayerAction::ActivateAbility`] action carries the index, so tests
//! and clients must pick the matching slot.
//!
//! [`PlayerAction::ActivateAbility`]: game_core::PlayerAction::ActivateAbility

use card_dsl::dsl::{activated, modify, Ability, Cost, ModifierScope, Stat};

/// `ArkhamDB` code for Physical Training (original-Core printing).
pub const CODE: &str = "01017";

/// Physical Training's two activated `[fast]` abilities.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 willpower for this skill test.
        activated(
            0,
            vec![Cost::Resources(1)],
            modify(Stat::Willpower, 1, ModifierScope::ThisSkillTest),
        ),
        // Index 1: +1 combat for this skill test.
        activated(
            0,
            vec![Cost::Resources(1)],
            modify(Stat::Combat, 1, ModifierScope::ThisSkillTest),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn abilities_are_two_fast_resource_costed_modifies() {
        let abilities = abilities();
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
    fn willpower_ability_pushes_willpower_for_this_skill_test() {
        assert!(matches!(
            abilities()[0].effect,
            Effect::Modify {
                stat: Stat::Willpower,
                delta: 1,
                scope: ModifierScope::ThisSkillTest,
            }
        ));
    }

    #[test]
    fn combat_ability_pushes_combat_for_this_skill_test() {
        assert!(matches!(
            abilities()[1].effect,
            Effect::Modify {
                stat: Stat::Combat,
                delta: 1,
                scope: ModifierScope::ThisSkillTest,
            }
        ));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
