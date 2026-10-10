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

use card_dsl::dsl::{self, Ability, LocationTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01037";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On play, discover 1 clue at the controller's location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::discover_clue(
        LocationTarget::YourLocation,
        1,
    ))]
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

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
