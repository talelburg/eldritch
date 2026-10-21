//! Heirloom of Hyperborea (Agnes Baker's signature asset, 01012).
//!
//! ```text
//! Item. Relic.
//! Agnes Baker deck only.
//! [reaction] After you play a [[Spell]] card: Draw 1 card.
//! ```
//!
//! **Cell: the `after` cell of the `CardPlayed` condition**, for the one
//! reaction, filtered to Spell cards played by the controller. The printed word
//! is *"After"*, and the ruling (<https://arkhamdb.com/card/01012>) fixes
//! when: *"This trigger occurs after
//! the Spell card has resolved in full."* — a Spell event once it is in the
//! discard pile, a Spell asset once it is in play.
//!
//! *"Agnes Baker deck only"* is a deckbuilding restriction, not an
//! ability.

use card_dsl::dsl::{self, Ability, CardFilter, EventPattern, EventTiming, InvestigatorTarget};

use crate::generated::traits::SPELL;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01012";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "`[reaction]` After you play a \[\[Spell\]\] card: Draw 1 card."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::CardPlayed {
            filter: CardFilter::with_trait(SPELL),
        },
        EventTiming::After,
        dsl::draw_cards(InvestigatorTarget::You, 1u8),
    )]
}
