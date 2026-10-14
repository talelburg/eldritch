//! Arcane Studies (Mystic asset, 01062).
//!
//! ```text
//! Talent.
//! [fast] Spend 1 resource: You get +1 [willpower] for this skill test.
//! [fast] Spend 1 resource: You get +1 [intellect] for this skill test.
//! ```
//!
//! Hyperawareness 01034's shape over willpower and intellect: two undesignated
//! `[fast]` activated abilities, each paying 1 resource for a `+1`
//! [`ModifierScope::ThisSkillTest`] modifier. No usage limit — the ruling
//! (<https://arkhamdb.com/card/01062>): *"You can use \[fast\]fast actions as
//! many times as you want, as long as you can pay the cost; there is no
//! limit."*
//!
//! The willpower ability is at `ability_index: 0`, the intellect ability at
//! `ability_index: 1`, in printed order.

use card_dsl::dsl::{self, Ability, Cost, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01062";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Arcane Studies' two activated `[fast]` abilities.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        // Index 0: +1 willpower for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Willpower, 1, ModifierScope::ThisSkillTest),
        ),
        // Index 1: +1 intellect for this skill test.
        dsl::activated(
            0,
            vec![Cost::Resources(1)],
            dsl::modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
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
