//! The card registry the client renders against, provided once at mount.
//!
//! The binary provides `cards::REGISTRY` (`main.rs`); a test provides the
//! registry it chooses. Components read it with [`use_cards`] and hand it to
//! the metadata helpers and `max_health` / `max_sanity`, so no render path
//! has a registry it didn't receive.

use game_core::card_registry::CardRegistry;
use leptos::prelude::{provide_context, use_context};

/// The context value carrying the card registry.
#[derive(Clone, Copy)]
struct Cards(&'static CardRegistry);

/// Provide `cards` to everything mounted under the current owner. Call it once,
/// inside the mount closure, before the app's views.
pub fn provide_cards(cards: &'static CardRegistry) {
    provide_context(Cards(cards));
}

/// The card registry provided at mount.
///
/// # Panics
///
/// Panics if no registry was provided — a mount that skipped
/// [`provide_cards`].
#[must_use]
pub fn use_cards() -> &'static CardRegistry {
    use_context::<Cards>()
        .expect("card registry provided at mount (`registry::provide_cards`)")
        .0
}
