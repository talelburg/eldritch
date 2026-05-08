//! Hand-implemented card effects.
//!
//! The card-data-pipeline emits *metadata* (name, cost, traits, …) for
//! every card in the snapshot, but the engine can't *play* a card
//! until someone writes its effect — either as a DSL declaration or a
//! Rust trait impl for the weird ones the DSL can't express.
//!
//! Each implemented card lives in its own submodule here. The registry
//! function below lists the codes of every implemented card; the
//! crate's [`is_playable`](super::is_playable) check looks them up.
//!
//! Phase 2 lands the registry framework; the first cards arrive in
//! subsequent PRs (Holy Rosary, Working a Hunch, etc.).

/// Returns the codes of all hand-implemented cards, sorted.
///
/// Used by [`super::is_playable`] to refuse decks containing
/// unimplemented cards (so we never let a player into a scenario
/// with cards the engine cannot resolve).
#[must_use]
pub fn implementations() -> &'static [&'static str] {
    // Sorted so binary search is valid.
    &[]
}
