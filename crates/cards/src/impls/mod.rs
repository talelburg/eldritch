//! Hand-implemented card effects.
//!
//! Each implemented card lives in its own submodule, exposing a
//! [`CODE`](holy_rosary::CODE) constant and an `abilities()` function
//! returning that card's [`Vec<Ability>`](card_dsl::dsl::Ability).
//!
//! The registry is the [`abilities_for`] dispatch — adding a card
//! means: drop a `crates/cards/src/impls/<name>.rs` file, declare the
//! `pub mod <name>;` here, and add a match arm in [`abilities_for`].
//! The crate's [`is_playable`](super::is_playable) check derives from
//! `abilities_for(code).is_some()`, so the two queries can never go
//! out of sync.
//!
//! # Module-naming convention
//!
//! Filenames are the card's lowercase snake-case name. When two
//! printings share a name (revised core / Chapter 2 reprints), the
//! later printings get a set suffix: `holy_rosary` for the original
//! core, `holy_rosary_rcore` if a revised-core variant lands. Codes
//! are the disambiguator at the registry level; filenames just stay
//! greppable.
//!
//! # Implemented so far
//!
//! - Holy Rosary (01059) — `Trigger::Constant` + unqualified
//!   `WhileInPlay`.
//! - Working a Hunch (01037) — `Trigger::OnPlay` + `DiscoverClue`.
//! - Magnifying Glass (01030) — `Trigger::Constant` +
//!   `WhileInPlayDuring(SkillTestKind::Investigate)`.
//! - Hyperawareness (01034) — two `Trigger::Activated { action_cost: 0 }`
//!   abilities with `Cost::Resources(1)` and `ThisSkillTest`-scoped
//!   `Modify`.
//! - Attic (01113) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `DealHorror(You, 1)`.
//! - Deduction (01039) — `Trigger::OnSkillTestResolution` (Success-
//!   gated) + `If(SkillTestKind(Investigate), DiscoverClue@TestedLocation)`.
//! - Roland Banks (01001) — investigator. `Trigger::OnEvent`
//!   reaction (`EnemyDefeated { by_controller: true }`, `After`) +
//!   `UsageLimit { count: 1, period: Round }` for "Limit once per
//!   round." Elder-sign half stubbed pending #118.
//!
//! The remaining Phase-3 card (Study #56) blocks on the
//! location-state shape.

use card_dsl::dsl::Ability;

pub mod attic;
pub mod deduction;
pub mod holy_rosary;
pub mod hyperawareness;
pub mod magnifying_glass;
pub mod roland_banks;
pub mod working_a_hunch;

/// Look up a card's hand-implemented abilities by code. Returns
/// `None` for unimplemented cards.
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    match code {
        attic::CODE => Some(attic::abilities()),
        deduction::CODE => Some(deduction::abilities()),
        holy_rosary::CODE => Some(holy_rosary::abilities()),
        hyperawareness::CODE => Some(hyperawareness::abilities()),
        magnifying_glass::CODE => Some(magnifying_glass::abilities()),
        roland_banks::CODE => Some(roland_banks::abilities()),
        working_a_hunch::CODE => Some(working_a_hunch::abilities()),
        _ => None,
    }
}
