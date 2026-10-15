//! Leather Coat (Survivor asset, 01072).
//!
//! ```text
//! Item. Armor.
//! Body. Health 2.
//! ```
//!
//! No text: the card is its Body slot and its 2 health, both printed
//! metadata (`CardKind::Asset { slots, health, .. }`). Soak is
//! engine-modelled from that metadata — while in play the coat can absorb
//! damage assigned to its controller, and is discarded once its health is
//! spent — so `abilities()` is empty, as for Holy Rosary 01059's soak half.
//! (An empty `abilities()` is still "implemented": `is_playable` keys off
//! registration, so the card passes the deck-import gate.) No rulings
//! (`data/arkhamdb-faq/no-rulings.txt`).

use card_dsl::dsl::Ability;

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01072";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// No ability — the card is entirely its slot and its damage soak.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
        assert!(crate::is_playable(super::CODE));
    }
}
