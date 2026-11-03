//! Fearless (Mystic skill, 01067).
//!
//! ```text
//! Innate.
//! 1 willpower icon.
//! If this skill test is successful, heal 1 horror.
//! ```
//!
//! Guts 01089's shape with a heal: the willpower icon is printed metadata, and
//! `abilities()` is one `OnSkillTestResolution` gated on `Success`, on any
//! test the card is committed to. Fearless prints no "Max 1 committed" line,
//! unlike Guts. Healing with no horror to heal is a no-op.
//!
//! Ruling (<https://arkhamdb.com/card/01067>): *"If you commit this card to
//! another player's skill test, and the test is successful, you heal 1 horror
//! (not the other player)."* `dsl::adds_result` makes the addition its
//! committer's, so `You` is whoever committed the card.

use card_dsl::dsl::{self, Ability, InvestigatorTarget, TestOutcome};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01067";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On any successful test this is committed to, heal 1 horror.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::adds_result(
        TestOutcome::Success,
        dsl::heal_horror(InvestigatorTarget::You, 1),
    )]
}
