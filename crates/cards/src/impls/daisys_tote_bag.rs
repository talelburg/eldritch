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
//!
//! [`Effect::AdditionalSlots`]: card_dsl::dsl::Effect::AdditionalSlots

use card_dsl::card_data::{CardType, Slot};
use card_dsl::dsl::{self, Ability, CardFilter};

use crate::generated::traits::TOME;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01008";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The bag's one printed ability: *"You have 2 additional hand slots, which
/// can only be used to hold \[\[Tome\]\] assets."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::constant(dsl::additional_slots(
        Slot::Hand,
        2,
        Some(CardFilter::with_trait(TOME).and_kind(CardType::Asset)),
    ))]
}
