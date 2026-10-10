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
//! The `health: 2, sanity: 2` soak the printed ally also carries is corpus
//! metadata; soak is engine-modeled from it (#44/K5 — nothing to declare in
//! `abilities()`; see `crates/cards/tests/non_attack_soak.rs`).

use card_dsl::dsl::{self, Ability, Cost, EnemyTarget, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Beat Cop (original-Core printing).
pub const CODE: &str = "01018";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Beat Cop's constant `+1 [combat]` and its `[fast]` discard-to-damage ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // You get +1 [combat] (while in play).
        dsl::constant(dsl::modify(Stat::Combat, 1, ModifierScope::WhileInPlay)),
        // [fast] Discard Beat Cop: Deal 1 damage to an enemy at your location.
        dsl::activated(
            0,
            vec![Cost::DiscardSelf],
            dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Choose, Effect, EntityScope, LocationSet, ModifierAudience, Trigger};

    use super::*;

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
                audience: ModifierAudience::Controller,
            }
        ));

        // Index 1: [fast] (action_cost 0), DiscardSelf cost, deal 1 to a chosen
        // enemy at your location.
        assert_eq!(
            abilities[1].trigger,
            Trigger::Activated {
                action_cost: 0,
                designator: None
            }
        );
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

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
