//! End-to-end test of the scenario-module wiring with the real
//! `scenarios::REGISTRY` installed.
//!
//! Drives the synthetic fixture through both of its acts via
//! `PlayerAction::AdvanceAct`; advancing past the terminal act latches
//! `GameState.resolution = Won { id: "demo" }`, and the push-model hook
//! emits `Event::ScenarioResolved` + runs `apply_resolution`. Lives in
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

use game_core::engine::apply;
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{GameState, InvestigatorId, Phase};
use game_core::{assert_event, Action, PlayerAction};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
    });
}

/// Drive a sequence of actions from an initial state, collecting all
/// events. Returns the final state and the concatenation of all event
/// vecs.
fn drive(initial_state: GameState, actions: Vec<Action>) -> (GameState, Vec<Event>) {
    let mut state = initial_state;
    let mut all_events = Vec::new();
    for action in actions {
        let result = apply(state, action);
        all_events.extend(result.events);
        state = result.state;
    }
    (state, all_events)
}

#[test]
fn synthetic_scenario_resolves_won_via_act_advance() {
    install_registry();
    let inv = InvestigatorId(1);
    let state = scenarios::test_fixtures::synthetic::setup();

    // StartScenario + close the mulligan window -> Investigation, round 1.
    let (mut state, _) = drive(
        state,
        vec![
            Action::Player(PlayerAction::StartScenario),
            Action::Player(PlayerAction::Mulligan {
                investigator: inv,
                indices_to_redraw: vec![],
            }),
        ],
    );
    assert_eq!(state.phase, Phase::Investigation);

    // Seed enough clues to advance both acts (2 + 2), then spend twice.
    state.investigators.get_mut(&inv).unwrap().clues = 4;
    let (state, events) = drive(
        state,
        vec![
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }), // act 0 -> 1
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }), // act 1 -> Won
        ],
    );

    assert_event!(
        events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
    assert!(state.resolution.is_some());
}
