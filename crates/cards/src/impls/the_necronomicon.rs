//! The Necronomicon (John Dee Translation), Daisy Walker's signature asset
//! weakness, 01009.
//!
//! ```text
//! Revelation - Put The Necronomicon into play in your threat area, with 3
//!   horror on it. It cannot leave play while it has 1 or more horror on it.
//! Treat each [elder_sign] you reveal on a chaos token as a [auto_fail].
//! [action]: Move 1 horror from The Necronomicon to Daisy Walker. Then, if The
//!   Necronomicon has no horror on it, discard it.
//! ```
//!
//! An asset weakness, so drawing it resolves its Revelation
//! (`glossary/Weakness.md`: *"When an investigator draws a weakness with a
//! player cardtype … resolve any Revelation effects on the card, and add it to
//! that investigator's hand"*). The Revelation puts it into play, so it never
//! reaches the hand. Fetching it with a search resolves the Revelation too: the
//! ruling (<https://arkhamdb.com/card/01009>) is *"If you add this to your hand
//! without drawing (e.g. via Research Librarian), the **Revelation** ability
//! still triggers."* Both paths run through the engine's drawn-weakness
//! resolution; this module only declares the Revelation. The 3 horror is
//! placed as the card enters (`Effect::PutIntoThreatArea`'s `horror`), not
//! dealt, so no harm event fires for it.
//!
//! # Module gap
//!
//! Only the Revelation's first sentence is implemented (#1090). Still missing,
//! each with its own ticket under #779:
//!
//! - *"It cannot leave play while it has 1 or more horror on it."* (#1093)
//! - *"Treat each `[elder_sign]` you reveal on a chaos token as a `[auto_fail]`."*
//!   (#1094)
//! - The `[action]` that moves 1 horror to Daisy Walker and discards the card
//!   once it is empty (#1095).
//!
//! Until they land, a Necronomicon in play has none of these effects: it
//! can be discarded, an elder sign resolves normally, and its 3 horror never
//! moves.

use card_dsl::dsl::{self, Ability};

use crate::impls::CardRecord;

/// `ArkhamDB` code for The Necronomicon.
pub const CODE: &str = "01009";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::put_into_threat_area_with_horror(
        CODE, 3,
    ))]
}
