//! Cunning Distraction (Survivor event, 01078).
//!
//! ```text
//! Evade. Automatically evade all enemies at your location.
//! ```
//!
//! **Designator: Evade, made automatically**
//! ([`ActionDesignator::AutomaticEvade`]). Playing it is an evade action for
//! every rule keyed on the kind, so it provokes no attack of opportunity, but
//! no skill test is made and no enemy need be engaged with you. The 01076
//! ruling (<https://arkhamdb.com/card/01076>), which 01078 carries too: *"if an
//! ability automatically evades 1 or more enemies, no skill test is made for
//! the evasion attempt whatsoever. Consequentially, because no skill test is
//! made, it is not considered a "successful" evasion."*
//!
//! The designator performs nothing; the evading is the ability's effect, a
//! fan-out over each enemy at your location. Its ruling
//! (<https://arkhamdb.com/card/01078>): *"This effect will evade and exhaust all
//! enemies in your location, even those that you're not engaged with
//! (***Aloof***, and those engaged with other players)."* So the set is every
//! enemy at the location, whoever it is engaged with. With none there the
//! effect cannot change the game state, and the card is not playable.
//!
//! Each enemy is evaded in turn, in enemy-id order, through the shared evade
//! exit, so each evasion is its own `EnemyEvaded` with its own "after"
//! reaction window: Pickpocketing 01046's draw lands after the first evasion,
//! before the second enemy is evaded. The text does not say whether "evade
//! all enemies" is one simultaneous evasion with one window, and no Core card
//! can tell the two apart; a card that can is the point to revisit.

use card_dsl::dsl::{self, Ability, ActionDesignator, EnemySet, EnemyTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Cunning Distraction.
pub const CODE: &str = "01078";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On play, an automatic Evade of each enemy at your location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play_as(
        ActionDesignator::AutomaticEvade,
        dsl::for_each(EnemySet::AtYourLocation, dsl::auto_evade(EnemyTarget::Each)),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, ForEachTargets, Trigger};

    use super::*;

    #[test]
    fn an_automatic_evade_of_each_enemy_at_your_location() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        let ability = &abilities[0];
        assert_eq!(ability.trigger, Trigger::OnPlay);
        assert_eq!(ability.designator, Some(ActionDesignator::AutomaticEvade));
        assert_eq!(
            ability.effect,
            Effect::ForEach {
                targets: ForEachTargets::Enemies(EnemySet::AtYourLocation),
                body: Box::new(Effect::AutoEvade {
                    target: EnemyTarget::Each,
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
