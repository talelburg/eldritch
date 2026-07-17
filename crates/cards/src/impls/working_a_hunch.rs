//! Working a Hunch (Seeker event, 01037).
//!
//! ```text
//! Fast. Play only during your turn.
//! Discover 1 clue at your location.
//! ```
//!
//! (Trait line: Insight.) The "Fast." keyword (no-action-cost-to-play)
//! is a card-level play-cost concern, not a DSL concern: it lives in
//! corpus metadata (`is_fast` / `play_only_during_turn`, pipeline-parsed)
//! and the play path consumes it (`metadata.is_fast()` in
//! `game-core`'s `play_card`; fast plays skip the action cost and are
//! offered in fast-play windows). `abilities()` only describes what the
//! `OnPlay` trigger does.

use card_dsl::dsl::{discover_clue, on_play, Ability, LocationTarget};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01037";

/// On play, discover 1 clue at the controller's location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(discover_clue(LocationTarget::YourLocation, 1))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, LocationTarget, Trigger};

    #[test]
    fn abilities_are_one_on_play_discover_clue() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
                count: 1,
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
