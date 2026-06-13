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
//! Zealot, scenario 1; Slice 1 C1a skeleton). The
//! [`synthetic`](test_fixtures::synthetic) fixture remains, gated behind
//! `test_fixtures`, as the minimal teaching example and the engine's
//! resolution-hook integration-test target. Further scenarios (the rest
//! of Night of the Zealot, Dunwich, …) land in later phases.

pub mod the_gathering;

#[cfg(any(test, feature = "test_fixtures"))]
pub mod test_fixtures;

use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};

/// Look up a scenario module by id. Returns `None` for ids not
/// known to this crate.
#[must_use]
pub fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        the_gathering::ID => Some(&the_gathering::MODULE),
        #[cfg(any(test, feature = "test_fixtures"))]
        test_fixtures::synthetic::ID => Some(&test_fixtures::synthetic::MODULE),
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
    fn module_for_resolves_synthetic() {
        let id = ScenarioId::new(test_fixtures::synthetic::ID);
        assert!(module_for(&id).is_some());
    }

    #[test]
    fn module_for_returns_none_for_unknown() {
        let id = ScenarioId::new("not-a-real-scenario");
        assert!(module_for(&id).is_none());
    }

    #[test]
    fn registry_dispatches_to_module_for() {
        let id = ScenarioId::new(test_fixtures::synthetic::ID);
        assert!((REGISTRY.module_for)(&id).is_some());
    }

    #[test]
    fn registry_resolves_the_gathering_and_rejects_unknown() {
        let id = ScenarioId::new(the_gathering::ID);
        assert!(
            (REGISTRY.module_for)(&id).is_some(),
            "the-gathering resolves"
        );
        let unknown = ScenarioId::new("does-not-exist");
        assert!(
            (REGISTRY.module_for)(&unknown).is_none(),
            "unknown id resolves to None"
        );
    }
}

#[cfg(test)]
mod setup_seeds_encounter_deck_tests {
    use super::test_fixtures::{synth_cards::SYNTH_TREACHERY_CODE, synthetic};
    use game_core::state::CardCode;

    #[test]
    fn synthetic_setup_seeds_encounter_deck_with_synth_treachery() {
        let state = synthetic::setup();
        assert_eq!(
            state.encounter_deck.len(),
            1,
            "synthetic fixture must seed exactly one encounter card",
        );
        assert_eq!(
            state.encounter_deck[0],
            CardCode(SYNTH_TREACHERY_CODE.into()),
        );
    }
}
