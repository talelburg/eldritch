//! Resolution with no scenario module registered.
//!
//! The tests that need a module's `apply_resolution` hook live in
//! `tests/resolution_module.rs`, which names one.

use super::*;
use crate::test_support::TEST_REGISTRIES;

const ADVANCE_ACT: TurnAction = TurnAction::AdvanceAct { investigator: INV };

/// `INV`'s turn menu in a scenario no module answers to, whose current (only)
/// act is terminal — it is the only card in the deck (ADR 0013) — and whose
/// investigator holds the one clue it takes to advance it. A single
/// `AdvanceAct` flips it and its reverse latches `Resolution(1)`, which is why
/// the test registry has to be named: the reverse is an ability the
/// registry serves, not a field on the deck entry.
fn terminal_act_state() -> GameState {
    // Open the turn so `legal_actions` enumerates `AdvanceAct`.
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(1))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new("unknown"))
        .build();
    // No builder setter for the act deck.
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

#[test]
fn resolution_event_fires_without_a_registered_module() {
    // No module registered for the scenario: the event still fires
    // (resolution is engine state), but apply_resolution can't run.
    let result = TestSession::new(terminal_act_state(), TEST_REGISTRIES)
        .take(&ADVANCE_ACT)
        .finish();
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "the scenario has ended"
    );
    assert_event!(result.events, Event::ScenarioResolved { .. });
}

#[test]
fn game_end_is_a_noop_without_a_game_end_ability_in_play() {
    // The GameEnd condition is emitted at resolution but is a
    // no-op with no card in play carrying a GameEnd ability: the resolution
    // still fires, and no TraumaSuffered is emitted.
    let result = TestSession::new(terminal_act_state(), TEST_REGISTRIES)
        .take(&ADVANCE_ACT)
        .finish();
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_no_event!(result.events, Event::TraumaSuffered { .. });
}
