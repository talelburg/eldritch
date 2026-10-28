//! "Look what I found!" (Survivor event, 01079).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fortune.
//! Fast. Play after you fail a skill test by 2 or less while investigating.
//! Discover 2 clues in your location.
//! ```
//!
//! **Cell: the `after` cell of the `SkillTestResolved` condition.** *"Play
//! after …"* names a timing point, which `glossary/Fast.md` reads as a
//! triggering condition for playing the card, so the event is a reaction on a
//! failed Investigate, played from hand there, as Evidence! 01022 is on a
//! defeat. *"by 2 or less"* is the pattern's [`MarginScope`].
//!
//! The ruling constrains the effect rather than the cell: *"You can only
//! 'discover' a clue if there is a clue on your location."*
//! (<https://arkhamdb.com/card/01079>). `Effect::DiscoverClue` discovers no
//! more than the location has, and is not offered at a location with none.
//!
//! The Fast/cost metadata comes from the generated corpus.

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, LocationTarget, MarginScope, SkillTestKind,
    TestOutcome, TestedLocationScope,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01079";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "Play after you fail a skill test by 2 or less while investigating. /
/// Discover 2 clues in your location."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::SkillTestResolved {
            outcome: TestOutcome::Failure,
            kind: Some(SkillTestKind::Investigate),
            by_controller: true,
            tested_location: TestedLocationScope::Any,
            margin: Some(MarginScope::at_most(2)),
        },
        EventTiming::After,
        dsl::discover_clue(LocationTarget::YourLocation, 2),
    )]
}
