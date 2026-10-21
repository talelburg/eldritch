//! Burglary (Rogue talent asset, 01045).
//!
//! ```text
//! [action] Exhaust Burglary: Investigate. If you succeed, instead of
//!   discovering clues, gain 3 resources.
//! ```
//!
//! One activated ability: an action, exhausting Burglary, that performs an
//! **Investigate** whose success gains 3 resources in place of the
//! clue discovery (the designator's `instead_of_discovery`).
//!
//! The replacement is the card's own, on its own investigation, so it is not a
//! general replacement effect, which the engine does not model beyond cancel.
//! The test is still a successful
//! investigation in every other respect: "after you successfully investigate"
//! (Dr. Milan Christopher 01033) fires at ST.6 as usual. Deduction 01039 only
//! raises the discovery's count, so it adds nothing here, which is its ruling
//! (<https://arkhamdb.com/card/01039>): *"does not stack with Burglary"*.
//!
//! Burglary's own ruling (<https://arkhamdb.com/card/01045>): *"You can use
//! Burglary even if there are no clues on your location."* The designator gate
//! asks only for a revealed location, never for clues, so that holds without
//! anything here.

use card_dsl::dsl::{self, Ability, Cost, InvestigatorTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Burglary (original-Core printing).
pub const CODE: &str = "01045";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Burglary's `[action] Exhaust: Investigate, gaining 3 resources instead of
/// discovering clues` ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        dsl::investigate(0i8).instead_of_discovery(dsl::gain_resources(InvestigatorTarget::You, 3)),
        1,
        vec![Cost::Exhaust],
        dsl::seq([]),
    )]
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
