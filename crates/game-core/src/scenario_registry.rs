//! Global scenario-registry binding for engine ↔ scenarios crate
//! lookups.
//!
//! Mirrors [`card_registry`](crate::card_registry): the engine needs
//! to look up a [`ScenarioModule`](crate::scenario::ScenarioModule) by
//! [`ScenarioId`](crate::scenario::ScenarioId) when checking
//! whether the current state has resolved, but `scenarios` depends on
//! `game-core` and not the other way around. This module bridges the
//! gap with a `OnceLock`-backed global.
//!
//! Hosts (server, test setup) call [`install`] exactly once with the
//! `scenarios::REGISTRY` constant. Engine code calls [`current`] when
//! it needs a lookup and treats `None` as "no scenario behavior wired
//! up; skip the resolution check."
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! Same reasoning as `card_registry`: the lookup interface is small
//! and fixed, the registry stays [`Copy`], and tests can construct
//! ad-hoc mock registries without touching the global.
//!
//! # Test isolation
//!
//! `OnceLock` is process-global, so tests that need a registry
//! installed run in their own integration-test binary (which is its
//! own process). Engine unit tests in `game-core` will exercise the
//! resolution-hook logic by **bypassing the global**: a subsequent
//! commit adds an engine helper that takes a `ScenarioRegistry`
//! parameter so tests can pass a locally-constructed mock. The one
//! test that exercises the global itself is the idempotent-install
//! test below, which is robust to running alongside other
//! global-touching tests.

use std::sync::OnceLock;

use crate::scenario::ScenarioRegistry;

static REGISTRY: OnceLock<ScenarioRegistry> = OnceLock::new();

/// Install the global scenario registry. Idempotent at the
/// `OnceLock` level: the first call wins; subsequent calls return
/// `Err` with the value the caller passed in.
///
/// Hosts call this once at startup. Tests that need real scenario
/// modules may call it from a `#[ctor]`-style helper or a
/// `LazyLock` initializer; double-install is harmless.
///
/// # Errors
///
/// Returns `Err(registry)` if a registry was already installed,
/// returning the value the caller passed in unchanged.
pub fn install(registry: ScenarioRegistry) -> Result<(), ScenarioRegistry> {
    REGISTRY.set(registry)
}

/// Get the installed registry, or `None` if no registry has been
/// installed yet. Engine code that needs a lookup should call this
/// and treat `None` as "no scenario behavior; skip" — the engine must
/// never panic on missing context.
#[must_use]
pub fn current() -> Option<&'static ScenarioRegistry> {
    REGISTRY.get()
}

#[cfg(test)]
mod tests {
    use super::{ScenarioRegistry, REGISTRY};
    use crate::event::Event;
    use crate::scenario::{Resolution, ScenarioId, ScenarioModule};
    use crate::state::GameState;
    use crate::test_support::TestGame;

    fn empty_state() -> GameState {
        TestGame::new().build()
    }

    fn no_op_apply(_res: &Resolution, _state: &mut GameState, _events: &mut Vec<Event>) {}

    static FAKE_MODULE: ScenarioModule = ScenarioModule {
        setup: empty_state,
        apply_resolution: no_op_apply,
    };

    fn fake_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "fake" {
            Some(&FAKE_MODULE)
        } else {
            None
        }
    }

    fn fake_registry() -> ScenarioRegistry {
        ScenarioRegistry {
            module_for: fake_module_for,
        }
    }

    #[test]
    fn module_for_returns_known_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("fake");
        let module = (reg.module_for)(&id).expect("known id should resolve");
        assert!(std::ptr::eq(module, std::ptr::addr_of!(FAKE_MODULE)));
    }

    #[test]
    fn module_for_returns_none_for_unknown_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("nonexistent");
        assert!((reg.module_for)(&id).is_none());
    }

    /// Process-global install — must run alongside other
    /// global-touching tests; we observe both outcomes to make the
    /// test robust to scheduling.
    #[test]
    fn install_is_idempotent_and_current_reflects_installed_value() {
        let first_attempt = super::install(fake_registry());
        let installed = super::current().expect("registry should be present after install");
        let id = ScenarioId::new("fake");
        let _ = (installed.module_for)(&id);
        if first_attempt.is_ok() {
            assert!(super::install(fake_registry()).is_err());
        }
        assert!(REGISTRY.get().is_some());
    }
}
