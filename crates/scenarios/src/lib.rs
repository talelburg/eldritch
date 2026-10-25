//! Scenarios and campaigns for Eldritch.
//!
//! Each scenario is a Rust module exposing `setup` and
//! `apply_resolution`. Campaigns orchestrate scenarios with branching
//! rules and a typed campaign log.
//!
//! # Engine integration
//!
//! The engine (in `game-core`) can't depend on this crate (cycle).
//! Engine code that needs a scenario lookup goes through a
//! [`game_core::scenario::ScenarioRegistry`]. This crate exposes [`REGISTRY`]
//! as a ready-made value that the host hands to every
//! [`game_core::engine::apply`].
//!
//! [`the_gathering`] (Night of the Zealot, scenario 1) is the one scenario
//! module. **Every module in this crate is shipped content** — there is no
//! test fixture to route around, so anything [`module_for`] resolves is a
//! scenario a player is meant to be able to start ([ADR 0016]). A test that
//! needs a scenario module builds its own shell locally, as
//! `crates/game-core/tests/scenario_resolution.rs` and
//! `crates/server/tests/common/mod.rs` do.
//!
//! [ADR 0016]: https://github.com/talelburg/eldritch/blob/main/docs/adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md

pub mod the_gathering;

#[cfg(feature = "test-support")]
use game_core::engine::Registries;
use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};

/// Look up a scenario module by id. Returns `None` for ids not
/// known to this crate.
#[must_use]
pub fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        the_gathering::ID => Some(&the_gathering::MODULE),
        _ => None,
    }
}

/// Ready-made [`ScenarioRegistry`] backed by this crate's scenario
/// modules. The host passes it to every [`game_core::engine::apply`]
/// inside a [`game_core::engine::Registries`].
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry { module_for };

/// The registries a scenario test names: [`cards::TEST_REGISTRY`] (the real
/// corpus with the synthetic test cards) paired with this crate's [`REGISTRY`].
///
/// Lives here because `cards` cannot see `scenarios`; a card test, which needs
/// no scenario module, names [`cards::TEST_REGISTRIES`].
#[cfg(feature = "test-support")]
pub const TEST_REGISTRIES: Registries = Registries {
    cards: &cards::TEST_REGISTRY,
    scenarios: &REGISTRY,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_for_returns_none_for_unknown() {
        let id = ScenarioId::new("not-a-real-scenario");
        assert!(module_for(&id).is_none());
    }

    /// Regression for #878: the toy scenario's id `"synthetic"` is not
    /// routable, so a `POST` of `{"scenario_id":"synthetic"}` cannot start a
    /// one-location demo on the production server. The fixture lives outside
    /// `src/`, which makes this structurally true; the test is insurance
    /// against a future re-export rather than the guarantee itself.
    #[test]
    fn module_for_returns_none_for_the_retired_synthetic_id() {
        let id = ScenarioId::new("synthetic");
        assert!(module_for(&id).is_none());
    }

    #[test]
    fn registry_resolves_the_gathering_and_rejects_unknown() {
        let id = ScenarioId::new(the_gathering::ID);
        let module = (REGISTRY.module_for)(&id).expect("the-gathering resolves");
        assert!(
            module.resolve_symbol.is_some(),
            "the-gathering must have a symbol hook"
        );
        let unknown = ScenarioId::new("does-not-exist");
        assert!(
            (REGISTRY.module_for)(&unknown).is_none(),
            "unknown id resolves to None"
        );
    }
}
