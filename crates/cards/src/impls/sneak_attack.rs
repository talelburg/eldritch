//! Sneak Attack (Rogue event, 01052).
//!
//! ```text
//! Deal 2 damage to an exhausted enemy at your location.
//! ```
//!
//! An `OnPlay` [`DealDamageToEnemy`] on an enemy chosen from one
//! [`EnemyScope`] with both constraints set: `at` your location and
//! `exhausted: Some(true)`. With no such enemy the change-state check proves
//! the effect inert, so the play is not offered. The damage is attributed to
//! the controller, like any card-dealt damage.
//!
//! Ruling (<https://arkhamdb.com/card/01052>): *"During your turn, you exhaust
//! an enemy by evading it, then play Sneak Attack. You do not play it during
//! the enemy phase."* Nothing to build for it: the event is not Fast, so it is
//! played only from the turn menu.
//!
//! [`DealDamageToEnemy`]: card_dsl::dsl::Effect::DealDamageToEnemy

use card_dsl::dsl::{self, Ability, Choose, EnemyScope, EnemyTarget, LocationSet};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01052";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On play, 2 damage to a chosen exhausted enemy at your location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::deal_damage_to_enemy(
        EnemyTarget::Chosen(Choose {
            scope: EnemyScope {
                at: LocationSet::Here,
                exhausted: Some(true),
                exempt: None,
            },
        }),
        2,
    ))]
}

#[cfg(test)]
mod tests {
    /// Catches a `CARD` record wired to the wrong code or abilities fn — the
    /// registry must dispatch CODE to this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
