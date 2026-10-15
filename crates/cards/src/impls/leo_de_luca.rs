//! Leo De Luca (Rogue ally asset, 01048).
//!
//! ```text
//! Leo De Luca. The Louisiana Lion.
//! Ally. Criminal.
//! You may take an additional action during your turn.
//! ```
//!
//! One constant: an unrestricted [`Effect::GainAdditionalAction`] scoped
//! [`AdditionalActionScope::WhileInPlay`]. The engine reads it off the card
//! each time it works out the investigator's actions, so the grant is gone the
//! moment Leo leaves play, and a second copy is a fresh instance that grants
//! again. His ruling (<https://arkhamdb.com/card/01048>): *"the first action you
//! spend each turn takes up the 'additional' designation. If Leo De Luca is
//! discarded mid-turn, you do not lose 1 action from your total pool of actions
//! for the turn. If you then play a second copy of the card, you gain a further
//! additional action."*
//!
//! The Ally slot and the `health: 2, sanity: 2` soak are corpus metadata, which
//! the engine models without anything declared here.
//!
//! [`Effect::GainAdditionalAction`]: card_dsl::dsl::Effect::GainAdditionalAction

use card_dsl::dsl::{self, Ability, AdditionalActionScope};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01048";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Leo's one printed ability: *"You may take an additional action during your
/// turn."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::constant(dsl::gain_additional_action(
        AdditionalActionScope::WhileInPlay,
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
