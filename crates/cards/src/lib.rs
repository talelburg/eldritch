//! Card definitions for Eldritch.
//!
//! Two layers:
//!
//! - **Metadata** — what's printed on a card (name, class, cost,
//!   traits, skill icons, …). The shape lives in
//!   [`card_dsl::card_data::CardMetadata`]; the corpus is generated
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
//! ready-made [`game_core::card_registry::CardRegistry`] value that the host
//! installs via [`game_core::card_registry::install`] before running
//! actions that touch card data.
//!
//! [`card-data-pipeline`]: ../../card_data_pipeline/index.html

use std::sync::OnceLock;

use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::Ability;
use game_core::card_registry::{CardRegistry, EligibilityFn, NativeConditionFn, NativeEffectFn};
use game_core::state::CardCode;

pub mod generated;
pub mod impls;

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
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    impls::abilities_for(code)
}

/// Whether a card has an effect implementation and can therefore be
/// taken into a scenario: whether it has a record in [`impls::ALL`], the
/// same list every registry lookup searches. Cards without an
/// implementation still appear in [`all`] (deckbuilding tools list them)
/// but are refused by the deck-import gate.
#[must_use]
pub fn is_playable(code: &str) -> bool {
    impls::record(code).is_some()
}

/// Adapter from [`CardCode`] to [`by_code`].
fn registry_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    by_code(code.as_str())
}

/// Adapter from [`CardCode`] to [`abilities_for`].
fn registry_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    abilities_for(code.as_str())
}

/// Adapter from [`CardCode`] to [`impls::back_abilities_for`] — the abilities
/// printed on a card's **reverse side**. See
/// [`game_core::card_registry::CardRegistry::back_abilities_for`] for when the
/// engine reads this side rather than the front.
fn registry_back_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    impls::back_abilities_for(code.as_str())
}

/// Adapter from a native-effect tag to its card-local handler.
fn registry_native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    impls::native_effect_for(tag)
}

/// Adapter from an eligibility tag to its card-local predicate.
fn registry_native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    impls::native_eligibility_for(tag)
}

/// Adapter from a [`Condition::Native`](card_dsl::dsl::Condition::Native) tag to
/// its card-local predicate.
fn registry_native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    impls::native_condition_for(tag)
}

/// Ready-made [`CardRegistry`] backed by this crate's corpus and
/// implementations. The host installs it once at startup with
/// [`game_core::card_registry::install`]; engine code then calls
/// [`game_core::card_registry::current`] when it needs a lookup.
pub const REGISTRY: CardRegistry = CardRegistry {
    metadata_for: registry_metadata_for,
    abilities_for: registry_abilities_for,
    back_abilities_for: registry_back_abilities_for,
    native_effect_for: registry_native_effect_for,
    native_eligibility_for: registry_native_eligibility_for,
    native_condition_for: registry_native_condition_for,
};

#[cfg(test)]
mod tests {
    use card_dsl::card_data::{CardType, Class};
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    use card_dsl::dsl::NativeKind;

    use super::*;
    use crate::impls::CardRecord;

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
        assert_eq!(mag.class(), Some(Class::Seeker));
        assert_eq!(mag.card_type(), CardType::Asset);
    }

    #[test]
    fn by_code_returns_none_for_unknown() {
        assert!(by_code("99999").is_none());
    }

    #[test]
    fn implemented_cards_are_playable() {
        // Phase-2 PR-I: Holy Rosary (01059), Working a Hunch (01037).
        // Phase-3 #37: Magnifying Glass (01030).
        // Phase-3 #38: Hyperawareness (01034).
        // Phase-3 #39: Deduction (01039).
        // Phase-3 #55: Roland Banks (01001).
        // Phase-7 C5d #239: .45 Automatic (01016), Physical Training
        // (01017), Machete (01020).
        assert!(is_playable("01037"));
        assert!(is_playable("01059"));
        assert!(is_playable("01030"));
        assert!(is_playable("01034"));
        assert!(is_playable("01039"));
        assert!(is_playable("01001"));
        assert!(is_playable("01016"));
        assert!(is_playable("01017"));
        assert!(is_playable("01020"));
    }

    #[test]
    fn unimplemented_cards_are_not_playable() {
        // Most of the corpus is still unimplemented. Fire Axe (02032)
        // is far-future Dunwich content (Phase 10), so it stays a
        // stable unimplemented canary as Core assets land.
        assert!(!is_playable("02032")); // Fire Axe (Dunwich, Phase 10)
        assert!(!is_playable("99999")); // unknown code
    }

    #[test]
    fn abilities_for_returns_some_for_implemented() {
        for code in ["01001", "01030", "01034", "01037", "01039", "01059"] {
            let abilities =
                abilities_for(code).unwrap_or_else(|| panic!("expected abilities for {code}"));
            assert!(
                !abilities.is_empty(),
                "abilities for {code} should be non-empty"
            );
        }
    }

    #[test]
    fn abilities_for_returns_none_for_unimplemented() {
        assert!(abilities_for("99999").is_none());
    }

    /// The `REGISTRY` constant must dispatch lookups to this crate's
    /// `by_code` / `abilities_for` — the whole point of the bridge.
    /// Holy Rosary (01059) is the canary because it has both metadata
    /// (Mystic asset) and an ability implementation.
    #[test]
    fn registry_constant_resolves_known_card() {
        let reg = REGISTRY;
        let code = CardCode::new("01059");
        let meta = (reg.metadata_for)(&code).expect("Holy Rosary should be in the corpus");
        assert_eq!(meta.code, "01059");
        assert_eq!(meta.name, "Holy Rosary");

        let abilities = (reg.abilities_for)(&code).expect("Holy Rosary should be playable");
        assert!(!abilities.is_empty());
    }

    // ---- the registration corpus (#986) ---------------------------------
    //
    // Each test walks every `impls::ALL` record and asserts what the engine
    // will see through `REGISTRY`, so a card left out of a namespace, a
    // misspelt tag or a module missing from `ALL` fails here rather than when
    // a player first plays the card.

    /// Every ability on both sides of `record`.
    fn record_abilities(record: &CardRecord) -> Vec<Ability> {
        let mut abilities = (record.abilities)();
        if let Some(back) = record.back_abilities {
            abilities.extend(back());
        }
        abilities
    }

    /// Every native tag `record` registers, by namespace.
    fn registered_tags(record: &CardRecord) -> Vec<(NativeKind, &'static str)> {
        let effects = record
            .native_effects
            .iter()
            .map(|(tag, _)| (NativeKind::Effect, *tag));
        let eligibility = record
            .native_eligibility
            .iter()
            .map(|(tag, _)| (NativeKind::Eligibility, *tag));
        let conditions = record
            .native_conditions
            .iter()
            .map(|(tag, _)| (NativeKind::Condition, *tag));
        effects.chain(eligibility).chain(conditions).collect()
    }

    /// Whether `tag` resolves through `REGISTRY`'s slot for `kind`.
    fn resolves(kind: NativeKind, tag: &str) -> bool {
        match kind {
            NativeKind::Effect => (REGISTRY.native_effect_for)(tag).is_some(),
            NativeKind::Eligibility => (REGISTRY.native_eligibility_for)(tag).is_some(),
            NativeKind::Condition => (REGISTRY.native_condition_for)(tag).is_some(),
        }
    }

    /// `<5-digit code>:<kebab-case>`, the tail being `[a-z0-9]+` words joined
    /// by single hyphens.
    fn is_well_formed_tag(tag: &str) -> bool {
        let Some((code, name)) = tag.split_once(':') else {
            return false;
        };
        code.len() == 5
            && code.bytes().all(|b| b.is_ascii_digit())
            && name.split('-').all(|word| {
                !word.is_empty()
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
    }

    #[test]
    fn every_referenced_native_tag_resolves_in_its_own_namespace() {
        for record in impls::ALL {
            for ability in record_abilities(record) {
                for native in ability.native_refs() {
                    assert!(
                        resolves(native.kind, native.tag),
                        "{}: {:?} tag {:?} is referenced but not registered in that namespace",
                        record.code,
                        native.kind,
                        native.tag,
                    );
                }
            }
        }
    }

    #[test]
    fn every_registered_tag_is_prefixed_with_its_own_code_and_kebab_case() {
        for record in impls::ALL {
            for (kind, tag) in registered_tags(record) {
                assert!(
                    tag.strip_prefix(record.code)
                        .is_some_and(|rest| rest.starts_with(':')),
                    "{}: {kind:?} tag {tag:?} is not prefixed with the card's own code",
                    record.code,
                );
                assert!(
                    is_well_formed_tag(tag),
                    "{}: {kind:?} tag {tag:?} is not `<code>:<kebab-case>`",
                    record.code,
                );
            }
        }
    }

    #[test]
    fn no_code_or_tag_is_registered_twice() {
        let mut codes = HashSet::new();
        let mut tags = HashSet::new();
        for record in impls::ALL {
            assert!(
                codes.insert(record.code),
                "{}: listed in ALL twice",
                record.code
            );
            for (kind, tag) in registered_tags(record) {
                assert!(
                    tags.insert((kind, tag)),
                    "{}: {kind:?} tag {tag:?} is registered twice",
                    record.code,
                );
            }
        }
    }

    #[test]
    fn every_registered_native_is_referenced_by_its_own_card() {
        for record in impls::ALL {
            let abilities = record_abilities(record);
            let referenced: Vec<_> = abilities.iter().flat_map(Ability::native_refs).collect();
            for (kind, tag) in registered_tags(record) {
                assert!(
                    referenced.iter().any(|r| r.kind == kind && r.tag == tag),
                    "{}: {kind:?} tag {tag:?} is registered but no ability of the card references it",
                    record.code,
                );
            }
        }
    }

    /// Reads `src/impls/` at test time: every card module's `CODE` must be in
    /// `ALL`. A module with no `pub mod` line is not compiled at all, so this
    /// also catches that half of the two edits.
    #[test]
    fn every_card_module_has_an_all_entry() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/impls");
        let mut modules = 0;
        for entry in fs::read_dir(&dir).expect("src/impls is readable") {
            let path = entry.expect("directory entry").path();
            let Some(module) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if path.extension().is_none_or(|e| e != "rs") || module == "mod" {
                continue;
            }
            let source = fs::read_to_string(&path).expect("card module is readable");
            let code = source
                .lines()
                .find_map(|line| line.strip_prefix("pub const CODE: &str = \""))
                .and_then(|rest| rest.strip_suffix("\";"))
                .unwrap_or_else(|| panic!("impls/{module}.rs declares no `pub const CODE`"));
            assert!(
                impls::record(code).is_some(),
                "impls/{module}.rs ({code}) has no entry in impls::ALL"
            );
            modules += 1;
        }
        assert_eq!(
            modules,
            impls::ALL.len(),
            "ALL lists a record no module file declares"
        );
    }

    #[test]
    fn an_unknown_code_or_tag_resolves_to_none_in_every_slot() {
        let code = CardCode::new("99999");
        assert!((REGISTRY.metadata_for)(&code).is_none());
        assert!((REGISTRY.abilities_for)(&code).is_none());
        assert!((REGISTRY.back_abilities_for)(&code).is_none());
        for kind in [
            NativeKind::Effect,
            NativeKind::Eligibility,
            NativeKind::Condition,
        ] {
            assert!(
                !resolves(kind, "99999:unknown"),
                "{kind:?} resolved an unknown tag"
            );
        }
    }
}
