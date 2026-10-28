//! Leo De Luca (Rogue ally asset, 01048).
//!
//! ```text
//! Leo De Luca. The Louisiana Lion.
//! Ally. Criminal.
//! You may take an additional action during your turn.
//! ```
//!
//! One constant: an unrestricted [`Effect::AdditionalActionWhileInPlay`]
//! marker. The engine reads it off the card
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
//! [`Effect::AdditionalActionWhileInPlay`]: card_dsl::dsl::Effect::AdditionalActionWhileInPlay

use card_dsl::dsl::{self, Ability};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01048";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Leo's one printed ability: *"You may take an additional action during your
/// turn."*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::constant(dsl::additional_action_while_in_play())]
}
