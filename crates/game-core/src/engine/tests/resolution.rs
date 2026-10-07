use super::*;

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
/// investigator holds exactly enough clues to advance it. A single
/// `AdvanceAct` flips it and its reverse latches `Resolution(1)`, which is
/// why the test registry has to be installed: the reverse is an ability the
/// registry serves, not a field on the deck entry.
fn terminal_act_state(scenario_id: Option<&str>) -> GameState {
    test_support::install_test_registry();
    let inv = InvestigatorId(1);
    let mut investigator = test_support::test_investigator(1);
    investigator.clues = 1;
    // Seat the open-turn frame (InvestigationPhase anchor + InvestigatorTurn)
    // so `legal_actions` enumerates `AdvanceAct` — the OptionId-routing entry
    // point these tests drive through `apply_with_scenario_registry`.
    let mut builder = GameStateBuilder::new()
        .with_investigator(investigator)
        .open_turn(inv);
    if let Some(id) = scenario_id {
        builder = builder.with_scenario_id(ScenarioId::new(id));
    }
    let mut state = builder.build();
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

/// Route an open-turn [`TurnAction`] through
/// [`apply_with_scenario_registry`] via `OptionId` (slice 2b, #447):
/// enumerate `legal_actions`, find the matching action's index, and submit it
/// as `ResolveInput(PickSingle)`. Mirrors `take_turn_action` but threads the
/// explicit registry the resolution-hook tests require (which the plain
/// `take_turn_action` helper cannot supply, as it calls `apply`).
fn take_turn_action_with_registry(
    state: GameState,
    action: &TurnAction,
    registry: Option<&ScenarioRegistry>,
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
        registry,
    )
}

#[test]
fn resolution_fires_and_applies_when_latch_set_with_module() {
    let state = terminal_act_state(Some("stamp"));
    let reg = ScenarioRegistry {
        module_for: stamp_module_for,
    };
    let result = take_turn_action_with_registry(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
        Some(&reg),
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::ScenarioResolved {
            ending: ScenarioEnding::Resolution(id)
        } if *id == ResolutionId::new(1)
    );
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].resources,
        99,
        "apply_resolution ran"
    );
}

#[test]
fn resolution_event_fires_without_a_registered_module() {
    // No registry: the event still fires (resolution is engine state),
    // but apply_resolution can't run.
    let state = terminal_act_state(Some("unknown"));
    let result = take_turn_action_with_registry(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
        None,
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::ScenarioResolved { .. });
}

#[test]
fn game_end_forced_point_is_noop_without_matching_cards() {
    // The GameEnd forced point (C5a #236) fires at resolution but is a
    // no-op with no controlled cards carrying a GameEnd ability: the
    // resolution still fires, and no TraumaSuffered is emitted.
    let state = terminal_act_state(Some("unknown"));
    let result = take_turn_action_with_registry(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
        None,
    );
    assert_event!(result.events, Event::ScenarioResolved { .. });
    assert_no_event!(result.events, Event::TraumaSuffered { .. });
}

#[test]
fn resolution_cancels_the_open_turn_and_does_not_refire() {
    // `terminal_act_state` seats the InvestigationPhase anchor + the
    // InvestigatorTurn frame (slice 1a / 2a-i, #393): AdvanceAct routes via
    // OptionId and latches the terminal act's `Won`.
    let state = terminal_act_state(Some("stamp"));
    let reg = ScenarioRegistry {
        module_for: stamp_module_for,
    };
    let first = take_turn_action_with_registry(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
        Some(&reg),
    );
    assert_event!(first.events, Event::ScenarioResolved { .. });
    // The scenario ended, so the framework sequence it was suspended in is
    // cancelled (#566): the open turn and its phase anchor are gone, and
    // with them every legal action. Before #566 the engine kept offering the
    // turn menu and cascading into the next round's Mythos phase.
    assert!(
        first.state.continuations.is_empty(),
        "the open turn and phase anchor are cancelled: {:?}",
        first.state.continuations,
    );
    assert!(legal_actions(&first.state).is_empty());

    // A later action finds no prompt outstanding and rejects; the
    // already-finished resolution does not re-fire.
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
    let inv = InvestigatorId(1);
    let mut state = terminal_act_state(Some("stamp"));
    state.investigators.get_mut(&inv).unwrap().clues = 0;
    // With 0 clues AdvanceAct is illegal, so it is not enumerated — there is
    // no OptionId to submit (OptionId-routing #447 expresses illegality as
    // non-enumeration).
    let legal = legal_actions(&state);
    assert!(!legal
        .iter()
        .any(|a| matches!(a, TurnAction::AdvanceAct { .. })));
    // Submitting an out-of-range OptionId rejects, and a Rejected outcome must
    // skip the resolution hook (no ScenarioResolved fires).
    let reg = ScenarioRegistry {
        module_for: stamp_module_for,
    };
    let result = apply_with_scenario_registry(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(legal.len()).unwrap())),
        }),
        Some(&reg),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::ScenarioResolved { .. });
}

#[test]
fn resolution_places_no_victory_without_qualifying_locations() {
    // No victory-bearing locations in play → nothing placed, no event,
    // no panic (covers the registry-absent / no-location path).
    let state = terminal_act_state(Some("stamp"));
    let reg = ScenarioRegistry {
        module_for: stamp_module_for,
    };
    let result = take_turn_action_with_registry(
        state,
        &TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        },
        Some(&reg),
    );
    assert!(result.state.victory_display.is_empty());
    assert_no_event!(result.events, Event::EnteredVictoryDisplay { .. });
}
