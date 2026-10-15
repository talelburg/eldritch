//! Dig Deep (Survivor asset, 01077).
//!
//! ```text
//! Talent.
//! [fast] Spend 1 resource: You get +1 [willpower] for this skill test.
//! [fast] Spend 1 resource: You get +1 [agility] for this skill test.
//! ```
//!
//! Hyperawareness 01034's shape with willpower in place of intellect: two
//! `[fast]` activated abilities, each paying [`Cost::Resources`]`(1)` for a
//! `+1` [`ModifierScope::ThisSkillTest`] push. The willpower ability is at
//! index 0 and the agility one at index 1, in printed order.
//!
//! Ruling (<https://arkhamdb.com/card/01077>): *"You can use \[fast\]fast
//! actions as many times as you want, as long as you can pay the cost; there
//! is no limit."* Neither ability declares a usage limit, so each is offered
//! again at the next player window for as long as a resource is left.

use card_dsl::dsl::{self, Ability, Cost, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01077";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Dig Deep's two `[fast]` abilities, in printed order.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 willpower for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Willpower, 1, ModifierScope::ThisSkillTest),
        ),
        // Index 1: +1 agility for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Agility, 1, ModifierScope::ThisSkillTest),
        ),
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
