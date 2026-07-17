//! Holy Rosary (Mystic asset, 01059).
//!
//! ```text
//! Hand. Item. Charm.
//! You get +1 [willpower].
//! ```
//!
//! # Horror soak
//!
//! Card metadata gives the asset `sanity: 2`. This is **not** a max-
//! sanity boost on the controller — it's the asset's horror-soak
//! capacity. Soak is engine-modeled from that corpus metadata
//! (#44/K5): while in play the asset can absorb horror assigned to
//! the controller (interactively distributed when contested), and is
//! discarded when its capacity is spent. Nothing about soak needs
//! declaring in `abilities()` — this impl ships the "+1 willpower
//! while in play" modifier; the soak half rides on metadata. See
//! `crates/cards/tests/non_attack_soak.rs` / `soak_distribution.rs`.

use card_dsl::dsl::{constant, modify, Ability, ModifierScope, Stat};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01059";

/// Holy Rosary's +1 willpower constant ability. The 2-horror-soak
/// capacity printed on the card is not yet modeled (see module doc).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![constant(modify(
        Stat::Willpower,
        1,
        ModifierScope::WhileInPlay,
    ))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, ModifierScope, Stat, Trigger};

    #[test]
    fn abilities_are_one_constant_willpower_modifier() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Constant);
        assert!(matches!(
            abilities[0].effect,
            Effect::Modify {
                stat: Stat::Willpower,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
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
