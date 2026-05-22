//! End-to-end test of the scenario-module wiring with the real
//! `scenarios::REGISTRY` installed.
//!
//! Drives `PlayerAction::StartScenario` against the synthetic
//! fixture and asserts `Event::ScenarioResolved` fires. Lives in
//! `crates/scenarios/tests/` rather than `game-core/src/engine/`
//! because:
//!
//! - The engine crate can't depend on `scenarios` (cycle direction
//!   is `game-core ← scenarios`).
//! - `scenario_registry::install` is process-global; an integration
//!   test binary gets its own process, so this install doesn't
//!   collide with `game-core`'s unit tests (which exercise the
//!   parameterized `apply_with_scenario_registry` helper instead).

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::Phase;
use game_core::{assert_event, Action, PlayerAction};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
    });
}

#[test]
fn synthetic_scenario_resolves_after_start_scenario() {
    install_registry();
    let state = scenarios::test_fixtures::synthetic::setup();
    let result = apply(state, Action::Player(PlayerAction::StartScenario));

    // StartScenario steps Mythos -> Investigation and bumps round to 1;
    // the synthetic fixture's detect_resolution fires on that condition.
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.round, 1);
    assert_event!(
        result.events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
}
