//! Scenarios and campaigns for Eldritch.
//!
//! Each scenario is a Rust module exposing `setup` and
//! `apply_resolution`. Campaigns orchestrate scenarios with branching
//! rules and a typed campaign log.
//!
//! # Engine integration
//!
//! The engine (in `game-core`) can't depend on this crate (cycle).
//! Engine code that needs a scenario lookup goes through
//! [`game_core::scenario_registry`]. This crate exposes [`REGISTRY`]
//! as a ready-made [`game_core::ScenarioRegistry`] value that the
//! host installs via
//! [`game_core::scenario_registry::install`]
//! before running actions that touch scenario data.
//!
//! [`the_gathering`] is the first real scenario module (Night of the
//! Zealot, scenario 1; Slice 1 C1a skeleton). **Every module in this
//! crate is shipped content** — there is no test fixture to route
//! around, so anything [`module_for`] resolves is a scenario a player
//! is meant to be able to start (#878, [ADR 0016]). A test that needs
//! a scenario module builds its own shell locally, as
//! `crates/game-core/tests/scenario_resolution.rs` and
//! `crates/server/tests/common/mod.rs` do. Further scenarios (the rest
//! of Night of the Zealot, Dunwich, …) land in later phases.
//!
//! [ADR 0016]: https://github.com/talelburg/eldritch/blob/main/docs/adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md

pub mod the_gathering;

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
/// modules. The host installs it once at startup with
/// [`game_core::scenario_registry::install`].
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry { module_for };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_for_returns_none_for_unknown() {
        let id = ScenarioId::new("not-a-real-scenario");
        assert!(module_for(&id).is_none());
    }

    /// The toy scenario used to be routable here whenever the
    /// `test_fixtures` feature was on — and it was on by default, so
    /// a `POST` of `{"scenario_id":"synthetic"}` started a one-location
    /// demo on the production server. #878 deleted the fixture, which
    /// makes this structurally true; the test is insurance against a
    /// future re-export rather than the guarantee itself.
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
