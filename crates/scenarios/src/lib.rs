//! Scenarios and campaigns for Eldritch.
//!
//! Each scenario is a Rust module exposing `setup`,
//! `detect_resolution`, and `apply_resolution`. Campaigns
//! orchestrate scenarios with branching rules and a typed campaign
//! log.
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
//! Phase-4 ships one module: the
//! [`synthetic`](test_fixtures::synthetic) fixture used by the
//! engine's resolution-hook integration test. Real scenarios (The
//! Gathering, Dunwich, …) land in subsequent phases.

#[cfg(any(test, feature = "test_fixtures"))]
pub mod test_fixtures;

#[cfg(any(test, feature = "test_fixtures"))]
use game_core::scenario::{ScenarioId, ScenarioModule, ScenarioRegistry};

/// Look up a scenario module by id. Returns `None` for ids not
/// known to this crate.
///
/// Gated behind `test_fixtures` for now — once a real scenario
/// (Phase 7 Gathering) lands, this becomes the unconditional
/// implementation.
#[cfg(any(test, feature = "test_fixtures"))]
#[must_use]
pub fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    match id.as_str() {
        test_fixtures::synthetic::ID => Some(&test_fixtures::synthetic::MODULE),
        _ => None,
    }
}

/// Ready-made [`ScenarioRegistry`] backed by this crate's scenario
/// modules. The host installs it once at startup with
/// [`game_core::scenario_registry::install`].
#[cfg(any(test, feature = "test_fixtures"))]
pub const REGISTRY: ScenarioRegistry = ScenarioRegistry { module_for };

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::scenario::ScenarioId;

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
