//! Hand-implemented card effects.
//!
//! Each implemented card lives in its own submodule, exposing a
//! [`CODE`](holy_rosary::CODE) constant and an `abilities()` function
//! returning that card's [`Vec<Ability>`](game_core::dsl::Ability).
//!
//! The registry is the [`abilities_for`] dispatch — adding a card
//! means: drop a `crates/cards/src/impls/<name>.rs` file, declare the
//! `pub mod <name>;` here, and add a match arm in [`abilities_for`].
//! The crate's [`is_playable`](super::is_playable) check derives from
//! `abilities_for(code).is_some()`, so the two queries can never go
//! out of sync.
//!
//! Phase 2 lands the framework + the three DSL-only cards. Activated-
//! ability cards (Hyperawareness) and triggered-effect cards
//! (Deduction) get separate Rust-impl placeholders in PR-J.

use game_core::dsl::Ability;

pub mod holy_rosary;
pub mod working_a_hunch;

/// Look up a card's hand-implemented abilities by code. Returns
/// `None` for unimplemented cards.
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    match code {
        holy_rosary::CODE => Some(holy_rosary::abilities()),
        working_a_hunch::CODE => Some(working_a_hunch::abilities()),
        _ => None,
    }
}
