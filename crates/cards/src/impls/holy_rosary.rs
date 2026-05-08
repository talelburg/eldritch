//! Holy Rosary (Mystic asset, 01059).
//!
//! ```text
//! Hand. Item. Charm.
//! You get +1 [willpower].
//! ```
//!
//! # Horror-soak gap
//!
//! Card metadata gives the asset `sanity: 2`. This is **not** a max-
//! sanity boost on the controller — it's the asset's horror-soak
//! capacity. While in play, horror that would hit the controller is
//! redirected to the asset; once it has absorbed 2 horror it's
//! discarded. That's a redirect-and-discard mechanic with state on
//! the asset itself, not a passive stat modifier.
//!
//! The DSL v0 doesn't model horror soak. This impl ships only the
//! "+1 willpower while in play" half. The soak half lands when the
//! DSL grows a soak / damage-redirect primitive (also unblocks Beat
//! Cop and other allies, which work the same way). The deck-import
//! gate will still treat Holy Rosary as playable on the strength of
//! the willpower ability; soak missing means the card is mechanically
//! weaker than printed in the simulator until the gap closes.

use game_core::dsl::{constant, modify, Ability, ModifierScope, Stat};

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
    use game_core::dsl::{Effect, ModifierScope, Stat, Trigger};

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
