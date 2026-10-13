use super::*;

const ADVANCE_ACT: TurnAction = TurnAction::AdvanceAct { investigator: INV };

/// `apply_resolution` that records it ran by stamping the acting
/// investigator's resources to a sentinel value, so tests can assert
/// the module hook (not just the event) fired.
fn stamp_apply(_ending: ScenarioEnding, state: &mut GameState, _events: &mut Vec<Event>) {
    if let Some(inv) = state.investigators.values_mut().next() {
        inv.resources = 99;
    }
}

fn unused_setup() -> GameState {
    GameStateBuilder::new().build()
}

static STAMP_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: unused_setup,
    apply_resolution: stamp_apply,
    layout: &[],
};

fn stamp_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    if id.as_str() == "stamp" {
        Some(&STAMP_MODULE)
    } else {
        None
    }
}

/// Build an Investigation-phase state whose current (only) act is
/// terminal — it is the only card in the deck (ADR 0013) — and whose
/// investigator holds `clues` clues: one is exactly enough to advance it. A
/// single `AdvanceAct` flips it and its reverse latches `Resolution(1)`,
/// which is why the test registry has to be installed: the reverse is an
/// ability the registry serves, not a field on the deck entry.
fn terminal_act_state(scenario_id: &str, clues: u8) -> GameState {
    // Open the turn so `legal_actions` enumerates `AdvanceAct`.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).clues(clues))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new(scenario_id))
        .build();
    // No builder setter for the act deck.
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

/// Take `action` from the turn menu through
/// [`apply_with_scenario_registry`], with `registry` as the scenario
/// registry.
///
/// An exemption from `TestSession::take`: the session reads the
/// process-global scenario registry, and a game-core unit-test binary
/// cannot install the stamp module there (installing in one test would
/// contaminate the others), so these tests thread an explicit registry. The
/// option id is the action's index on the menu, as `take` computes it.
fn take_turn_action_with_registry(
    state: GameState,
    action: &TurnAction,
    registry: ScenarioRegistry,
) -> ApplyResult {
    let actions = legal_actions(&state);
    let idx = actions
        .iter()
        .position(|a| a == action)
        .unwrap_or_else(|| panic!("{action:?} not offered; legal: {actions:?}"));
    apply_with_scenario_registry(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
        Some(&registry),
    )
}

fn stamp_registry() -> ScenarioRegistry {
    ScenarioRegistry {
        module_for: stamp_module_for,
    }
}

#[test]
fn resolution_fires_and_applies_when_latch_set_with_module() {
    let result = take_turn_action_with_registry(
        terminal_act_state("stamp", 1),
        &ADVANCE_ACT,
        stamp_registry(),
    );
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "the scenario has ended"
    );
    assert_event!(
        result.events,
        Event::ScenarioResolved {
            ending: ScenarioEnding::Resolution(id)
        } if *id == ResolutionId::new(1)
    );
    assert_eq!(
        result.state.investigators[&INV].resources, 99,
        "apply_resolution ran"
    );
}

#[test]
fn resolution_event_fires_without_a_registered_module() {
    // No module registered for the scenario: the event still fires
    // (resolution is engine state), but apply_resolution can't run.
    let result = TestSession::new(terminal_act_state("unknown", 1))
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
    // The GameEnd condition (C5a #236) is emitted at resolution but is a
    // no-op with no card in play carrying a GameEnd ability: the resolution
    // still fires, and no TraumaSuffered is emitted.
    let result = TestSession::new(terminal_act_state("unknown", 1))
        .take(&ADVANCE_ACT)
        .finish();
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_no_event!(result.events, Event::TraumaSuffered { .. });
}

#[test]
fn resolution_cancels_the_open_turn_and_does_not_refire() {
    // `terminal_act_state` opens the turn: AdvanceAct latches the terminal
    // act's resolution.
    let reg = stamp_registry();
    let first = take_turn_action_with_registry(terminal_act_state("stamp", 1), &ADVANCE_ACT, reg);
    assert_event!(first.events, Event::ScenarioResolved { .. });
    // The scenario ended, so the framework sequence it was suspended in is
    // cancelled (#566): the open turn and its phase anchor are gone, and
    // with them every legal action. Before #566 the engine kept offering the
    // turn menu and cascading into the next round's Mythos phase. A stack
    // invariant: no frame survives the ending.
    assert!(
        first.state.continuations.is_empty(),
        "the open turn and phase anchor are cancelled: {:?}",
        first.state.continuations,
    );
    assert!(legal_actions(&first.state).is_empty());

    // A later answer finds no prompt outstanding and rejects; the
    // already-finished resolution does not re-fire. The subject is that
    // stray wire action itself, so it is sent raw.
    let second = apply_with_scenario_registry(
        first.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
        Some(&reg),
    );
    assert!(matches!(second.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(second.events, Event::ScenarioResolved { .. });
}

#[test]
fn resolution_skipped_on_rejected_outcome() {
    let state = terminal_act_state("stamp", 0);
    // With 0 clues AdvanceAct is illegal, so it is not enumerated — there is
    // no OptionId to submit (OptionId-routing #447 expresses illegality as
    // non-enumeration).
    let legal = legal_actions(&state);
    assert!(!legal
        .iter()
        .any(|a| matches!(a, TurnAction::AdvanceAct { .. })));
    // Submitting an out-of-range OptionId rejects, and a Rejected outcome must
    // skip the resolution hook (no ScenarioResolved fires). The subject is
    // that malformed wire action itself, so it is sent raw.
    let result = apply_with_scenario_registry(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(legal.len()).unwrap())),
        }),
        Some(&stamp_registry()),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::ScenarioResolved { .. });
}

#[test]
fn resolution_places_no_victory_without_qualifying_locations() {
    // No victory-bearing locations in play → nothing placed, no event,
    // no panic (covers the registry-absent / no-location path).
    let result = take_turn_action_with_registry(
        terminal_act_state("stamp", 1),
        &ADVANCE_ACT,
        stamp_registry(),
    );
    assert!(result.state.victory_display.is_empty());
    assert_no_event!(result.events, Event::EnteredVictoryDisplay { .. });
}
