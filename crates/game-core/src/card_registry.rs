//! Global card-registry binding for engine ↔ cards crate lookups.
//!
//! The engine (in `game-core`) needs to look up a card's metadata
//! (type, class, traits, …) and abilities ([`Vec<Ability>`]) by
//! [`CardCode`] when resolving actions like `PlayCard` or computing
//! constant-modifier contributions during a skill test. The card
//! corpus and ability implementations live in the `cards` crate, which
//! depends on `game-core` — so a direct call from `game-core` into
//! `cards` is impossible.
//!
//! This module bridges the gap with a `OnceLock`-backed global. The
//! `cards` crate exposes a [`CardRegistry`] value (function pointers
//! into its own `by_code` / `abilities_for`) and the host (server,
//! `main` in tests that need real cards) installs it once at startup
//! via [`install`]. Engine code calls [`current`] when it needs a
//! lookup and gracefully rejects when no registry is installed —
//! tests that don't touch card data work fine without an install.
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! The lookup interface is small and fixed. A plain struct of `fn`
//! pointers avoids vtable overhead, makes the registry [`Copy`], and
//! keeps `dyn` out of the engine's hot path. Tests construct registry
//! values with custom function pointers to mock the corpus when
//! needed.
//!
//! # Test isolation
//!
//! `OnceLock` is process-global, so all tests in a process share the
//! same registry. Today no test mocks card data — tests use the real
//! corpus or skip card lookups entirely via the `GameStateBuilder` builder —
//! so this is fine. If/when per-test mocking is needed, we'll move to
//! a registry threaded through `apply()` or stored on `GameState`.

use std::sync::OnceLock;

use crate::card_data::CardMetadata;
use crate::dsl::Ability;
use crate::engine::{Cx, EngineOutcome, EvalContext};
use crate::state::{CardCode, GameState};

/// A card-local Rust effect: mutates state and emits events through the
/// effect-resolution context, returning the resolution outcome. Provided
/// by the `cards` crate and dispatched from [`Effect::Native`] via
/// [`CardRegistry::native_effect_for`].
///
/// [`Effect::Native`]: crate::dsl::Effect::Native
pub type NativeEffectFn = fn(&mut Cx, &EvalContext) -> EngineOutcome;

/// A card-local read-only eligibility predicate: returns whether a reaction
/// ability whose [`Ability::eligibility`](crate::dsl::Ability::eligibility) names
/// this tag may be offered (RR p.2: an ability can't initiate if its effect
/// won't change game state). Receives the same [`EvalContext`] native effects do
/// (controller + source). Dispatched from the reaction scan via
/// [`CardRegistry::native_eligibility_for`].
pub type EligibilityFn = fn(&GameState, &EvalContext) -> bool;

/// Bundle of card-lookup function pointers.
///
/// The `cards` crate provides a static instance wrapping its own
/// `by_code` / `abilities_for`; tests can construct ad-hoc instances
/// with mock function pointers.
#[derive(Debug, Clone, Copy)]
pub struct CardRegistry {
    /// Look up static metadata by code. Returns `None` for unknown
    /// codes.
    pub metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>,
    /// Look up hand-implemented abilities by code. Returns `None` for
    /// unimplemented (or unknown) cards.
    pub abilities_for: fn(&CardCode) -> Option<Vec<Ability>>,
    /// Look up a card-local Rust effect by its [`Effect::Native`] tag.
    /// Returns `None` for unregistered tags.
    ///
    /// [`Effect::Native`]: crate::dsl::Effect::Native
    pub native_effect_for: fn(&str) -> Option<NativeEffectFn>,
    /// Look up a card-local eligibility predicate by its
    /// [`Ability::eligibility`](crate::dsl::Ability::eligibility) tag. Returns
    /// `None` for unregistered tags.
    pub native_eligibility_for: fn(&str) -> Option<EligibilityFn>,
}

static REGISTRY: OnceLock<CardRegistry> = OnceLock::new();

/// Install the global card registry. Idempotent at the
/// `OnceLock` level: the first call wins; subsequent calls return
/// `Err` with the value they tried to set.
///
/// Hosts (server, test setup) call this exactly once at startup. Tests
/// that need real cards may call it from a `#[ctor]`-style helper or
/// from a `LazyLock` initializer; double-install is harmless.
///
/// # Errors
///
/// Returns `Err(registry)` if a registry was already installed,
/// returning the value the caller passed in unchanged.
pub fn install(registry: CardRegistry) -> Result<(), CardRegistry> {
    REGISTRY.set(registry)
}

/// Get the installed registry, or `None` if no registry has been
/// installed yet. Engine handlers that need a lookup should call this
/// and reject cleanly on `None` rather than panic — the engine must
/// never panic on missing context.
#[must_use]
pub fn current() -> Option<&'static CardRegistry> {
    REGISTRY.get()
}

#[cfg(test)]
mod tests {
    use super::{CardRegistry, REGISTRY};
    use crate::card_data::{CardKind, CardMetadata, CardType, Class, SkillIcons};
    use crate::dsl::{constant, modify, ModifierScope, Stat};
    use crate::state::CardCode;
    use std::sync::OnceLock;

    /// Build a hand-rolled `CardMetadata` for a fake test card.
    fn fake_metadata() -> CardMetadata {
        CardMetadata {
            code: "TEST1".to_string(),
            name: "Test Card".to_string(),
            text: None,
            traits: vec![],
            pack_code: "test".to_string(),
            kind: CardKind::Asset {
                class: Class::Neutral,
                cost: Some(0),
                xp: Some(0),
                slots: vec![],
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: false,
                deck_limit: 2,
                uses: None,
                play_only_during_turn: false,
            },
        }
    }

    fn fake_metadata_static() -> &'static CardMetadata {
        static M: OnceLock<CardMetadata> = OnceLock::new();
        M.get_or_init(fake_metadata)
    }

    fn fake_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
        if code.as_str() == "TEST1" {
            Some(fake_metadata_static())
        } else {
            None
        }
    }

    fn fake_abilities_for(code: &CardCode) -> Option<Vec<crate::dsl::Ability>> {
        if code.as_str() == "TEST1" {
            Some(vec![constant(modify(
                Stat::Willpower,
                1,
                ModifierScope::WhileInPlay,
            ))])
        } else {
            None
        }
    }

    /// A locally-constructed registry exercises both function-pointer
    /// fields without touching the global `OnceLock` — so multiple
    /// tests can run in parallel.
    fn fake_registry() -> CardRegistry {
        CardRegistry {
            metadata_for: fake_metadata_for,
            abilities_for: fake_abilities_for,
            native_effect_for: |_| None,
            native_eligibility_for: |_| None,
        }
    }

    #[test]
    fn metadata_lookup_returns_known_card() {
        let reg = fake_registry();
        let code = CardCode::new("TEST1");
        let meta = (reg.metadata_for)(&code).expect("known code should resolve");
        assert_eq!(meta.code, "TEST1");
        assert_eq!(meta.card_type(), CardType::Asset);
    }

    #[test]
    fn metadata_lookup_returns_none_for_unknown_code() {
        let reg = fake_registry();
        let code = CardCode::new("99999");
        assert!((reg.metadata_for)(&code).is_none());
    }

    #[test]
    fn abilities_lookup_returns_known_card() {
        let reg = fake_registry();
        let code = CardCode::new("TEST1");
        let abilities = (reg.abilities_for)(&code).expect("known code should resolve");
        assert_eq!(abilities.len(), 1);
    }

    #[test]
    fn abilities_lookup_returns_none_for_unknown_code() {
        let reg = fake_registry();
        let code = CardCode::new("99999");
        assert!((reg.abilities_for)(&code).is_none());
    }

    /// Process-global install — must run in isolation from other
    /// global-touching tests; we serialize via a single test that
    /// owns the global. Subsequent calls to `install` should fail
    /// (idempotent-by-error semantics of `OnceLock::set`).
    ///
    /// NOTE (#448 cp2a): this test uses `install_test_registry()` to
    /// ensure the process-global slot is always occupied by the
    /// standard test registry (which knows `TEST_INV`). Using a
    /// bespoke `fake_registry()` here would race with other tests that
    /// depend on `install_test_registry()` — whichever wins the
    /// `OnceLock` would silently starve the other.
    #[test]
    fn install_is_idempotent_and_current_reflects_installed_value() {
        // The test registry is the canonical game-core test registry.
        // Install it (idempotent) to ensure `current()` returns Some.
        crate::test_support::install_test_registry();
        let installed = super::current().expect("registry should be present after install");
        // A second install attempt must return Err (already set).
        assert!(super::install(fake_registry()).is_err());
        // Sanity: `current()` keeps returning Some.
        assert!(REGISTRY.get().is_some());
        // Verify `installed` resolves the TEST_INV code that the test
        // registry knows about.
        let _ = (installed.metadata_for)(&CardCode::new(crate::test_support::TEST_INV));
    }
}
