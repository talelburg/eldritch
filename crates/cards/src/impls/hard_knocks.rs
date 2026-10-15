//! Hard Knocks (Rogue asset, 01049).
//!
//! ```text
//! Talent.
//! [fast] Spend 1 resource: You get +1 [combat] for this skill test.
//! [fast] Spend 1 resource: You get +1 [agility] for this skill test.
//! ```
//!
//! Hyperawareness 01034's shape over combat and agility: two undesignated
//! `[fast]` activated abilities, each paying 1 resource for a `+1`
//! [`ModifierScope::ThisSkillTest`] modifier. No usage limit — the ruling
//! (<https://arkhamdb.com/card/01049>): *"You can use \[fast\]fast actions as
//! many times as you want, as long as you can pay the cost; there is no
//! limit."*
//!
//! The combat ability is at `ability_index: 0`, the agility ability at
//! `ability_index: 1`, in printed order.

use card_dsl::dsl::{self, Ability, Cost, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01049";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Hard Knocks' two activated `[fast]` abilities.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 combat for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Combat, 1, ModifierScope::ThisSkillTest),
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
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
