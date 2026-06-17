//! Beat Cop (Guardian ally asset, 01018).
//!
//! ```text
//! You get +1 [combat].
//! [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
//! ```
//!
//! Two abilities: a constant `+1 [combat]` while in play, and a `[fast]`
//! ability whose cost discards the ally itself (`Cost::DiscardSelf`, #301)
//! to deal 1 direct damage to a chosen enemy at the controller's location
//! (`Effect::DealDamageToEnemy` over the keystone's
//! `EnemyTarget::Chosen(At(your location))`, #349/#301). The activation
//! pre-cost check rejects when no enemy is there, so the discard is never
//! paid for nothing; 2+ enemies suspend for the controller's pick.
//!
//! The `sanity: 2` horror-soak the printed ally also carries is corpus
//! metadata, not modeled as an ability (tracked in #44).

use card_dsl::dsl::{
    activated, constant, deal_damage_to_enemy, modify, Ability, Cost, EnemyTarget, ModifierScope,
    Stat,
};

/// `ArkhamDB` code for Beat Cop (original-Core printing).
pub const CODE: &str = "01018";

/// Beat Cop's constant `+1 [combat]` and its `[fast]` discard-to-damage ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // You get +1 [combat] (while in play).
        constant(modify(Stat::Combat, 1, ModifierScope::WhileInPlay)),
        // [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
        activated(
            0,
            vec![Cost::DiscardSelf],
            deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Choose, Effect, EntityScope, LocationSet, Trigger};

    #[test]
    fn abilities_are_constant_combat_plus_fast_discard_damage() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 2);

        // Index 0: constant +1 combat while in play.
        assert_eq!(abilities[0].trigger, Trigger::Constant);
        assert!(matches!(
            abilities[0].effect,
            Effect::Modify {
                stat: Stat::Combat,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
            }
        ));

        // Index 1: [fast] (action_cost 0), DiscardSelf cost, deal 1 to a chosen
        // enemy at your location.
        assert_eq!(abilities[1].trigger, Trigger::Activated { action_cost: 0 });
        assert_eq!(abilities[1].costs, vec![Cost::DiscardSelf]);
        assert!(matches!(
            abilities[1].effect,
            Effect::DealDamageToEnemy {
                target: EnemyTarget::Chosen(Choose {
                    scope: EntityScope::At(LocationSet::Here),
                }),
                amount: 1,
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
