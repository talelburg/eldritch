//! Magnifying Glass (Seeker asset, 01030).
//!
//! ```text
//! Hand. Item. Tool.
//! Fast.
//! You get +1 [intellect] while investigating.
//! ```
//!
//! # Fast
//!
//! The "Fast" keyword means the card costs no action to play. This is
//! a play-cost concern, not a DSL concern: the corpus carries a
//! card-level `is_fast` flag (pipeline-parsed) and the play path
//! consumes it — fast plays skip the action charge and are offered in
//! fast-play windows (`play_card` / `enumerate_fast_plays` in
//! `game-core`).
//!
//! # Why `WhileInPlayDuring`, not `WhileInPlay`
//!
//! The bonus is qualified ("while investigating"). A bare
//! `Modify(Intellect, +1, WhileInPlay)` would add +1 to every
//! intellect test — including treacheries that test intellect to
//! resist (Frozen in Fear, Crypt Chill, …) — which is wrong per the
//! rules. `WhileInPlayDuring(SkillTestKind::Investigate)` (#45) gates
//! the contribution to the Investigate action's intellect test.

use card_dsl::dsl::{constant, modify, Ability, ModifierScope, SkillTestKind, Stat};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01030";

/// Magnifying Glass's +1 intellect while investigating constant ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![constant(modify(
        Stat::Intellect,
        1,
        ModifierScope::WhileInPlayDuring(SkillTestKind::Investigate),
    ))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, ModifierScope, SkillTestKind, Stat, Trigger};

    #[test]
    fn abilities_are_one_constant_intellect_while_investigating_modifier() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Constant);
        assert!(matches!(
            abilities[0].effect,
            Effect::Modify {
                stat: Stat::Intellect,
                delta: 1,
                scope: ModifierScope::WhileInPlayDuring(SkillTestKind::Investigate),
            }
        ));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
