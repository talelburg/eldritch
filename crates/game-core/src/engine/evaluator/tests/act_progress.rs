use super::*;

#[test]
fn advance_current_act_non_terminal_bumps_cursor() {
    let mut state = GameStateBuilder::new()
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.act_deck = vec![
        Act {
            code: CardCode("a1".into()),
            clue_threshold: 0,
        },
        Act {
            code: CardCode("a2".into()),
            clue_threshold: 0,
        },
    ];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::AdvanceCurrentAct,
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    // The advance is deferred to an AdvanceReverse frame (#482); drive it
    // (no registry ⇒ the reverse fires nothing ⇒ it drives straight through).
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(state.act_index, 1);
    assert!(state.ending.is_none());
}

/// `AdvanceCurrentAct` on a **terminal** act (01110's shape) advances it
/// like any other — the cursor stays because there is no next card, and the
/// ending comes from the reverse the advance fires, not from the effect
/// (ADR 0013).
#[test]
fn advance_current_act_on_a_terminal_act_lets_its_reverse_end_the_scenario() {
    test_support::install_test_registry();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 0,
    }];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::AdvanceCurrentAct,
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(state.act_index, 0, "terminal act does not move the cursor");
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
    );
}

/// `Effect::ReachResolution(n)` latches `Resolution(n)` and does nothing
/// else: the DSL carries the bare printed number and the newtype conversion
/// happens here (ADR 0013). No deck is modeled, because the effect does not
/// care which card ran it.
#[test]
fn reach_resolution_latches_the_printed_resolution_point() {
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = run(
        &mut cx,
        &Effect::ReachResolution(3),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(3)))
    );
    assert!(events.is_empty(), "the latch emits nothing itself");
}

/// It latches through `end_scenario`, so a resolution point already reached
/// this scenario stands (ADR 0004's first-writer-wins).
#[test]
fn reach_resolution_does_not_overwrite_an_ending_already_latched() {
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    run(&mut cx, &Effect::ReachResolution(1), ctx);
    run(&mut cx, &Effect::ReachResolution(2), ctx);
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
    );
}
