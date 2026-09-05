//! Synthetic test cards used by Phase-4's integration tests.
//!
//! These don't exist in any printed pack — they're vehicles for
//! proving engine wiring end-to-end without depending on real corpus
//! cards. The card codes use an underscore prefix (`_synth_*`) to
//! guarantee no collision with `ArkhamDB`'s digit-prefixed codes.
//!
//! Exposed alongside [`TEST_REGISTRY`] — integration tests install
//! this registry so the on-draw path resolves against synthetic cards
//! that are guaranteed not to collide with real `ArkhamDB` codes
//! (underscore-prefix), rather than depending on a specific corpus
//! card existing. The `cards` crate is still compiled in as a
//! workspace dep — what `TEST_REGISTRY` isolates is the *runtime*
//! registry lookup, not the compile-time footprint.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata};
use game_core::card_registry::CardRegistry;
use game_core::dsl::{gain_resources, revelation, Ability, InvestigatorTarget};
use game_core::state::CardCode;

/// Code for the synthetic location the demo fixture stamps onto its one
/// location. Underscore prefix guarantees no collision with `ArkhamDB`'s
/// digit-prefixed real codes. Referenced from
/// [`crate::test_fixtures::synthetic::setup`].
pub const SYNTH_LOC_CODE: &str = "_synth_loc";

/// Code for the synthetic treachery. Underscore prefix guarantees no
/// collision with `ArkhamDB`'s digit-prefixed five-char codes.
pub const SYNTH_TREACHERY_CODE: &str = "_synth_treachery";

/// Static metadata for the synthetic treachery. Only `code`/`name`/the
/// `Treachery` kind carry meaning for the tests.
fn synth_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_TREACHERY_CODE.to_owned(),
        name: "Synthetic Treachery".to_owned(),
        text: Some("Revelation - You gain 1 resource. (Synthetic; not a printed card.)".to_owned()),
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".to_owned(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_treachery_metadata)
}

/// `metadata_for` function pointer used by [`TEST_REGISTRY`].
///
/// Falls through to `game_core::test_support::metadata_for_test_inv` for
/// the synthetic investigator code (`TEST_INV`) used by `test_investigator()`.
/// After cp2a `max_health()`/`max_sanity()` read from the registry; the
/// fallthrough makes capacity reads work without installing a separate registry.
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        _ => game_core::test_support::metadata_for_test_inv(code),
    }
}

/// `abilities_for` function pointer used by [`TEST_REGISTRY`].
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(vec![revelation(gain_resources(InvestigatorTarget::You, 1))]),
        // The synthetic terminal act/agenda cards are composed in from
        // `game_core::test_support` the way `metadata_for_test_inv` is above:
        // the `synthetic` fixture's decks end in one, and without its reverse a
        // terminal advance would reach no ending (ADR 0013).
        _ => game_core::test_support::abilities_for_terminal(code),
    }
}

/// Ready-made [`CardRegistry`] backed by this module's synthetic
/// cards. Integration tests install this via
/// [`game_core::card_registry::install`] instead of `cards::REGISTRY`
/// so they don't pull in the full corpus.
///
/// Process-isolated: each `cargo test --test` binary gets its own
/// process, so this install doesn't collide with `cards::REGISTRY`
/// installs in other test binaries.
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
    ..CardRegistry::EMPTY
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_resolves_synth_treachery() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let meta = metadata_for(&code).expect("synth treachery must resolve");
        assert_eq!(meta.code, SYNTH_TREACHERY_CODE);
        assert_eq!(meta.card_type(), game_core::card_data::CardType::Treachery);
    }

    #[test]
    fn metadata_for_returns_none_for_unknown_code() {
        let code = CardCode("not_in_synth_registry".into());
        assert!(metadata_for(&code).is_none());
    }

    #[test]
    fn abilities_for_returns_one_revelation_ability() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let abilities = abilities_for(&code).expect("synth treachery must have abilities");
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, game_core::dsl::Trigger::Revelation,);
    }

    #[test]
    fn test_registry_dispatches_to_module_functions() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        assert!((TEST_REGISTRY.metadata_for)(&code).is_some());
        assert!((TEST_REGISTRY.abilities_for)(&code).is_some());
    }
}
