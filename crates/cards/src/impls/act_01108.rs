//! Trapped (The Gathering Act 1, 01108).
//!
//! ```text
//! Act 1 — Trapped. Clues: 2.
//! (reverse) Put into play the set-aside Hallway, Cellar, Attic, and
//! Parlor. Discard each enemy in the Study. Place each investigator in
//! the Hallway. Remove the Study from the game.
//! ```
//!
//! The reverse side is a Forced on-advance ability: it fires via
//! `ForcedTriggerPoint::ActAdvanced` when the act advances, before the
//! next act becomes current. "Discard each enemy in the Study" is a
//! faithful **no-op** — nothing can spawn into the isolated Act-1 Study
//! in Slice-1 scope (location reveal-on-entry is TODO(#257); no encounter
//! path targets the Study). The set-aside locations + their connections
//! are built by the scenario's `setup()`; this ability just moves them
//! into play, relocates investigators to the Hallway (01112), and removes
//! the Study (01111).

use card_dsl::dsl::{
    on_event, put_set_aside_locations_into_play, relocate_all_investigators,
    remove_location_from_game, Ability, Effect, EventPattern, EventTiming,
};

/// `ArkhamDB` code for Act 1, "Trapped".
pub const CODE: &str = "01108";

/// 01108's Forced on-advance reverse: build the Act-1 board.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::ActAdvanced,
        EventTiming::After,
        Effect::Seq(vec![
            put_set_aside_locations_into_play(),
            relocate_all_investigators("01112"), // the Hallway
            remove_location_from_game("01111"),  // the Study
        ]),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_are_one_forced_on_advance_world_build() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After
            }
        );
        let Effect::Seq(steps) = &abilities[0].effect else {
            panic!("expected a Seq, got {:?}", abilities[0].effect);
        };
        assert!(matches!(steps[0], Effect::PutSetAsideLocationsIntoPlay));
        assert!(matches!(&steps[1], Effect::RelocateAllInvestigators { to } if to == "01112"));
        assert!(
            matches!(&steps[2], Effect::RemoveLocationFromGame { location } if location == "01111")
        );
    }
}
