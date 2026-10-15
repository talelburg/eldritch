//! Daisy's Tote Bag (Seeker signature asset, 01008).
//!
//! ```text
//! Daisy's Tote Bag.
//! Item.
//! Daisy Walker deck only.
//! You have 2 additional hand slots, which can only be used to hold [[Tome]] assets.
//! ```
//!
//! One constant: an [`Effect::AdditionalSlots`] marker granting a Tome-only
//! pool of two hand slots. The engine reads it off the card each time it
//! counts the investigator's slots, so the pool is gone the moment the bag
//! leaves play. The bag itself has no slot symbol, so it holds no slot. Its
//! ruling (<https://arkhamdb.com/card/01008>) is about signature cards in
//! deckbuilding only, and "Daisy Walker deck only." is a deckbuilding
//! restriction the engine does not police.

use card_dsl::card_data::{CardType, Slot};
use card_dsl::dsl::{self, Ability, CardFilter, Effect};

use crate::generated::traits::TOME;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01008";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The bag's one printed ability: *"You have 2 additional hand slots, which
/// can only be used to hold [[Tome]] assets."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::constant(Effect::AdditionalSlots {
        slot: Slot::Hand,
        count: 2,
        only: Some(CardFilter::with_trait(TOME).and_kind(CardType::Asset)),
    })]
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
