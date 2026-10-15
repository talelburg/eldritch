//! Daisy Walker (Seeker investigator, 01002).
//!
//! ```text
//! Daisy Walker. The Librarian.
//! Miskatonic.
//! Willpower 3. Intellect 5. Combat 2. Agility 2.
//! Health 5. Sanity 9.
//!
//! You may take an additional action during your turn, which can only be
//! used on [[Tome]] [action] abilities.
//! [elder_sign] effect: +0. If you succeed, draw 1 card for each [[Tome]]
//! you control.
//! ```
//!
//! # Scope
//!
//! The additional action is a constant
//! [`Effect::AdditionalActionWhileInPlay`] on her investigator card, restricted
//! by [`AdditionalActionRestriction::ActivatedAbilityOn`] to the Tome trait. The
//! engine reads it each turn, and only an `[action]` ability on a Tome she has
//! in play can use it.
//!
//! Her ruling (<https://arkhamdb.com/card/01002>): *"If you are instructed to
//! lose 1 or more actions, you have that many fewer actions to take during
//! your turn. This is referring to your normal three “full” actions. So if you
//! are instructed to lose 1 or more actions, those must be the ones that are
//! “lost” first. If you have no more of those actions to lose, then you start
//! losing “additional” actions, of your choice."* The engine already loses
//! standard actions first.
//!
//! # Module gap
//!
//! The elder sign is registered as its `+0` only. *"If you succeed, draw 1 card
//! for each \[\[Tome\]\] you control"* is #1091.
//!
//! [`Effect::AdditionalActionWhileInPlay`]: card_dsl::dsl::Effect::AdditionalActionWhileInPlay

use card_dsl::dsl::{self, Ability, AdditionalActionRestriction, CardFilter, IntExpr};

use crate::generated::traits::TOME;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01002";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Daisy's two printed abilities:
///
/// - "You may take an additional action during your turn, which can only be
///   used on \[\[Tome\]\] \[action\] abilities."
/// - `[elder_sign]` effect: "+0." (the draw is #1091)
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::constant(dsl::restricted_additional_action_while_in_play(
            AdditionalActionRestriction::ActivatedAbilityOn(CardFilter::with_trait(TOME)),
        )),
        dsl::elder_sign(IntExpr::Lit(0)),
    ]
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
