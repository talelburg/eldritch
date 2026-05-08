//! Working a Hunch (Seeker event, 01037).
//!
//! > Insight. Fast.
//! > Discover 1 clue at your location.
//!
//! The "Fast." keyword (no-action-cost-to-play) is a card-level play
//! cost concern, not a DSL concern. It'll be encoded on the card
//! declaration alongside `abilities()` once the play-cost layer
//! exists; for now `abilities()` only describes what the `OnPlay`
//! trigger does.

use game_core::dsl::{discover_clue, on_play, Ability, LocationTarget};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01037";

/// On play, discover 1 clue at the controller's location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(discover_clue(
        LocationTarget::ControllerLocation,
        1,
    ))]
}

#[cfg(test)]
mod tests {
    use game_core::dsl::{Effect, LocationTarget, Trigger};

    #[test]
    fn abilities_are_one_on_play_discover_clue() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::ControllerLocation,
                count: 1,
            }
        ));
    }
}
