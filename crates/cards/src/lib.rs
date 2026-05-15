//! Card definitions for Eldritch.
//!
//! Two layers:
//!
//! - **Metadata** — what's printed on a card (name, class, cost,
//!   traits, skill icons, …). The shape lives in
//!   [`game_core::card_data::CardMetadata`]; the corpus is generated
//!   by the [`card-data-pipeline`] CLI from the pinned `ArkhamDB`
//!   snapshot at `data/arkhamdb-snapshot/`. The generated constants
//!   live in [`generated`]; access them via [`all`] or [`by_code`].
//!
//! - **Effects** — what a card *does* during play. Hand-written, one
//!   submodule per card under [`impls`]. The DSL handles common
//!   patterns; weird cards get a Rust trait impl.
//!
//! A card is *playable* iff it has an effect implementation. The
//! [`is_playable`] check is what the deck importer (Phase 9) uses to
//! refuse decks containing unimplemented cards.
//!
//! # Engine integration
//!
//! The engine (in `game-core`) can't import this crate directly —
//! that would cycle. Engine code that needs card lookups (`PlayCard`,
//! constant-modifier queries during skill tests, …) goes through
//! [`game_core::card_registry`]. This crate exposes [`REGISTRY`] as a
//! ready-made [`game_core::CardRegistry`] value that the host
//! installs via [`game_core::card_registry::install`] before running
//! actions that touch card data.
//!
//! [`card-data-pipeline`]: ../../card_data_pipeline/index.html

use std::sync::OnceLock;

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::state::CardCode;

pub mod generated;
pub mod impls;

pub use game_core::card_data::{CardType, Class, SkillIcons, Slot};

/// All card metadata in the Eldritch corpus, lazily initialized on
/// first access. Sorted by [`CardMetadata::code`].
pub fn all() -> &'static [CardMetadata] {
    static ALL: OnceLock<Vec<CardMetadata>> = OnceLock::new();
    ALL.get_or_init(generated::all_cards).as_slice()
}

/// Look up a card by its `ArkhamDB` code. O(log n) via binary search.
#[must_use]
pub fn by_code(code: &str) -> Option<&'static CardMetadata> {
    all()
        .binary_search_by(|c| c.code.as_str().cmp(code))
        .ok()
        .map(|i| &all()[i])
}

/// Look up a card's hand-implemented abilities by code. Returns
/// `None` for unimplemented cards. Re-exported from
/// [`impls::abilities_for`].
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<game_core::dsl::Ability>> {
    impls::abilities_for(code)
}

/// Whether a card has an effect implementation and can therefore be
/// taken into a scenario. Derived from [`abilities_for`] so the two
/// queries can never go out of sync. Cards without an implementation
/// still appear in [`all`] (deckbuilding tools list them) but are
/// refused by the deck-import gate.
#[must_use]
pub fn is_playable(code: &str) -> bool {
    abilities_for(code).is_some()
}

/// Adapter from [`CardCode`] to [`by_code`].
fn registry_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    by_code(code.as_str())
}

/// Adapter from [`CardCode`] to [`abilities_for`].
fn registry_abilities_for(code: &CardCode) -> Option<Vec<game_core::dsl::Ability>> {
    abilities_for(code.as_str())
}

/// Ready-made [`CardRegistry`] backed by this crate's corpus and
/// implementations. The host installs it once at startup with
/// [`game_core::card_registry::install`]; engine code then calls
/// [`game_core::card_registry::current`] when it needs a lookup.
pub const REGISTRY: CardRegistry = CardRegistry {
    metadata_for: registry_metadata_for,
    abilities_for: registry_abilities_for,
};

#[cfg(test)]
mod tests {
    use super::{all, by_code, is_playable, CardType, Class};

    #[test]
    fn corpus_is_sorted_by_code() {
        let cards = all();
        let mut prev = "";
        for c in cards {
            assert!(
                c.code.as_str() > prev,
                "cards must be sorted unique by code; saw {prev:?} then {:?}",
                c.code
            );
            prev = c.code.as_str();
        }
    }

    #[test]
    fn by_code_finds_a_known_card() {
        // 01030 is Magnifying Glass (the basic Seeker hand-slot tool
        // from the original Core Set). Used here as a sanity-check
        // that the generated corpus is non-empty and indexable.
        let mag = by_code("01030").expect("Magnifying Glass should exist");
        assert_eq!(mag.name, "Magnifying Glass");
        assert_eq!(mag.class, Class::Seeker);
        assert_eq!(mag.card_type, CardType::Asset);
    }

    #[test]
    fn by_code_returns_none_for_unknown() {
        assert!(by_code("99999").is_none());
    }

    #[test]
    fn implemented_cards_are_playable() {
        // Phase-2 PR-I: Holy Rosary (01059), Working a Hunch (01037).
        // Phase-3 #37: Magnifying Glass (01030) once #45 (per-skill-
        // test scope) landed.
        assert!(is_playable("01037"));
        assert!(is_playable("01059"));
        assert!(is_playable("01030"));
    }

    #[test]
    fn unimplemented_cards_are_not_playable() {
        // Hyperawareness (01034) needs Trigger::Activated + cost
        // primitives (#53). Deduction (01039) needs the skill-test
        // commit window (#63) or the after-resolution trigger (#64).
        assert!(!is_playable("01034"));
        assert!(!is_playable("01039"));
        // Phase-3 doesn't touch most of the corpus.
        assert!(!is_playable("01001")); // Roland Banks
        assert!(!is_playable("99999")); // unknown code
    }

    #[test]
    fn abilities_for_returns_some_for_implemented() {
        for code in ["01030", "01037", "01059"] {
            let abilities = super::abilities_for(code)
                .unwrap_or_else(|| panic!("expected abilities for {code}"));
            assert!(
                !abilities.is_empty(),
                "abilities for {code} should be non-empty"
            );
        }
    }

    #[test]
    fn abilities_for_returns_none_for_unimplemented() {
        assert!(super::abilities_for("99999").is_none());
    }

    /// The `REGISTRY` constant must dispatch lookups to this crate's
    /// `by_code` / `abilities_for` — the whole point of the bridge.
    /// Holy Rosary (01059) is the canary because it has both metadata
    /// (Mystic asset) and an ability implementation.
    #[test]
    fn registry_constant_resolves_known_card() {
        use game_core::state::CardCode;

        let reg = super::REGISTRY;
        let code = CardCode::new("01059");
        let meta = (reg.metadata_for)(&code).expect("Holy Rosary should be in the corpus");
        assert_eq!(meta.code, "01059");
        assert_eq!(meta.name, "Holy Rosary");

        let abilities = (reg.abilities_for)(&code).expect("Holy Rosary should be playable");
        assert!(!abilities.is_empty());
    }

    #[test]
    fn registry_constant_returns_none_for_unknown_code() {
        use game_core::state::CardCode;

        let reg = super::REGISTRY;
        let code = CardCode::new("99999");
        assert!((reg.metadata_for)(&code).is_none());
        assert!((reg.abilities_for)(&code).is_none());
    }
}
