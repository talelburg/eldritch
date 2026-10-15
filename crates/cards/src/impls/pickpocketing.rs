//! Pickpocketing (Rogue talent asset, 01046).
//!
//! ```text
//! Talent. Illicit.
//! [reaction] After you evade an enemy, exhaust Pickpocketing: Draw 1 card.
//! ```
//!
//! **Cell: the `after` cell of the `EnemyEvaded` condition**, for the one
//! reaction. The printed word is *"After"*, and the condition's resolution is
//! the evasion itself, so the window opens with the enemy already disengaged
//! and exhausted. The condition is announced by the one evade exit every way
//! of evading reaches, which the ruling (<https://arkhamdb.com/card/01046>)
//! needs: *"the auto-evade effect from Breaking and Entering can trigger the
//! reaction on Pickpocketing (0)"*.
//!
//! *"exhaust Pickpocketing"* is the ability's cost, paid as it initiates; an
//! exhausted Pickpocketing cannot pay it, so is not offered.
//!
//! # Module gap
//!
//! TODO(#1082): the other ruling — *"If you use this to draw a card with the
//! same triggering condition (e.g. Close Call), you can play that card during
//! the same reaction window."* — is not honoured: a reaction window never adds
//! an option once it is open.

use card_dsl::dsl::{self, Ability, Cost, EventPattern, EventTiming, InvestigatorTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Pickpocketing (original-Core printing).
pub const CODE: &str = "01046";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "`[reaction]` After you evade an enemy, exhaust Pickpocketing: Draw 1
/// card."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::EnemyEvaded,
        EventTiming::After,
        dsl::draw_cards(InvestigatorTarget::You, 1),
    )
    .with_costs(vec![Cost::Exhaust])]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
