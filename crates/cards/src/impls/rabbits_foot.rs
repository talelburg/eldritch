//! Rabbit's Foot (Survivor asset, 01075).
//!
//! ```text
//! Item. Charm.
//! [reaction] After you fail a skill test, exhaust Rabbit's Foot: Draw 1 card.
//! ```
//!
//! **Cell: the `after` cell of the `SkillTestResolved` condition**, for the one
//! reaction. The printed word is *"After"*, and the condition's resolution is
//! the test's outcome, so the window opens with the failure settled. *"you
//! fail"* is `by_controller` on a failure of any kind, at any location.
//!
//! *"exhaust Rabbit's Foot"* is the ability's cost, paid as it initiates; an
//! exhausted Rabbit's Foot cannot pay it, so is not offered.
//!
//! # Module gap
//!
//! TODO(#1082): the ruling (<https://arkhamdb.com/card/01075>) — *"If you use
//! this to draw a card with the same triggering condition (e.g. "Look what I
//! found!"), you can play that card during the same reaction window."* — is
//! not honoured: a reaction window never adds an option once it is open.

use card_dsl::dsl::{
    self, Ability, Cost, EventPattern, EventTiming, InvestigatorTarget, TestOutcome,
    TestedLocationScope,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01075";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "`[reaction]` After you fail a skill test, exhaust Rabbit's Foot: Draw 1
/// card."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::SkillTestResolved {
            outcome: TestOutcome::Failure,
            kind: None,
            by_controller: true,
            tested_location: TestedLocationScope::Any,
        },
        EventTiming::After,
        dsl::draw_cards(InvestigatorTarget::You, 1u8),
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
