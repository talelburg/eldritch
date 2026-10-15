//! Stray Cat (Survivor ally asset, 01076).
//!
//! ```text
//! [fast] Discard Stray Cat: Automatically evade a non-[[Elite]] enemy at your
//! location.
//! ```
//!
//! An **automatic evasion** (`GLOSSARY.md`): no skill test is made, so it is
//! not a *successful* evasion, but the enemy is evaded all the same, exhausted
//! if ready and its engagement broken. The 01076 ruling
//! (<https://arkhamdb.com/card/01076>): *"if an ability automatically evades 1
//! or more enemies, no skill test is made for the evasion attempt whatsoever.
//! Consequentially, because no skill test is made, it is not considered a
//! "successful" evasion."* It is the shared evade exit, so "after you evade"
//! reactions still fire (Pickpocketing 01046's ruling,
//! <https://arkhamdb.com/card/01046>).
//!
//! The target is any enemy at your location whose card is not Elite, engaged
//! or not — the other 01076 ruling: *"You can use Stray Cat to evade an enemy
//! that you're not engaged with."* It is typed
//! ([`Effect::AutoEvade`](card_dsl::dsl::Effect::AutoEvade) over a chosen
//! [`EnemyScope`] exempting Elite), so the activation's pre-cost check refuses
//! it when no such enemy is there, before the discard is paid, as Beat Cop
//! 01018's is. One candidate is evaded with no prompt; two or more ask.
//!
//! The `health: 1` soak the printed ally carries is corpus metadata, which the
//! engine models from it.

use card_dsl::dsl::{
    self, Ability, CardFilter, Choose, Cost, EnemyScope, EnemyTarget, LocationSet,
};

use crate::generated::traits::ELITE;
use crate::impls::CardRecord;

/// `ArkhamDB` code for Stray Cat.
pub const CODE: &str = "01076";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Stray Cat's `[fast]` discard-to-evade ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated(
        0,
        vec![Cost::DiscardSelf],
        dsl::auto_evade(EnemyTarget::Chosen(Choose {
            scope: EnemyScope::at(LocationSet::Here).except(CardFilter::with_trait(ELITE)),
        })),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    use super::*;

    #[test]
    fn a_fast_discard_self_ability_evading_a_chosen_non_elite_enemy_here() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let ability = &abilities[0];
        assert_eq!(ability.trigger, Trigger::Activated { action_cost: 0 });
        assert_eq!(ability.designator, None);
        assert_eq!(ability.costs, vec![Cost::DiscardSelf]);
        assert_eq!(
            ability.effect,
            Effect::AutoEvade {
                target: EnemyTarget::Chosen(Choose {
                    scope: EnemyScope {
                        at: LocationSet::Here,
                        exhausted: None,
                        exempt: Some(CardFilter::with_trait("Elite")),
                    },
                }),
            }
        );
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
