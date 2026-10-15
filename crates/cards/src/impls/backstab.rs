//! Backstab (Rogue event, 01051).
//!
//! ```text
//! Fight. This attack uses [agility] instead of [combat]. This attack deals
//! +2 damage.
//! ```
//!
//! **Designator: Fight**, on the event's `OnPlay` ability. Playing it
//! **performs a fight action** against an enemy engaged with you, modified as
//! the card prints: it tests `[agility]` in place of `[combat]` (the
//! designator's skill override) and deals `+2` damage on a hit. The ability's
//! own effect is empty.
//!
//! The override swaps only the skill, not the value it is tested against.
//! Ruling (<https://arkhamdb.com/card/01051>): *"Backstab makes you test your
//! Agility skill instead of your Combat skill against the enemy's Fight value
//! (not its Evade value)."* The Fight grounding reads the enemy's fight value
//! whatever skill the designator tests.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{self, Ability};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01051";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On play, a Fight testing `[agility]` in place of `[combat]` that deals
/// `+2` damage.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play_as(
        dsl::fight(0i8, 2u8).testing(SkillKind::Agility),
        dsl::seq([]),
    )]
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
