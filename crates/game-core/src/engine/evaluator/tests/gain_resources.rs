use super::*;

#[test]
fn gain_resources_increments_target_wallet_and_emits_event() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let resources_before = state.investigators[&id].resources;
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::You, 3),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, resources_before + 3);
    assert_event!(
        events,
        Event::ResourcesGained { investigator, amount: 3 } if *investigator == id
    );
}

/// `push_effect` + the real `drive` runs an effect to completion identically
/// to the (deleted in Slice D) synchronous `apply_effect`: the root frame is
/// pushed, the global loop steps it, the effect applies, the frame pops.
#[test]
fn push_effect_then_drive_runs_to_completion() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let resources_before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    push_effect(
        &mut cx,
        &dsl::gain_resources(InvestigatorTarget::You, 3),
        ctx(1),
    );
    assert!(
        matches!(cx.state.continuations.last(), Some(Continuation::Effect(_))),
        "the effect root frame is pushed for the loop",
    );

    let out = dispatch::drive(&mut cx, EngineOutcome::Done);
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, resources_before + 3);
    assert!(state.continuations.is_empty(), "effect frame popped");
}

#[test]
fn gain_resources_zero_amount_is_a_silent_noop() {
    // Symmetric with discover_clue_on_empty_location_is_a_silent_noop:
    // a zero-amount gain isn't a state change. Crucially, it also
    // skips target resolution, so an `Active` target with no
    // active investigator doesn't reject for amount=0.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let resources_before = state.investigators[&id].resources;
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::Active, 0),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, resources_before);
    assert!(events.is_empty());
}

#[test]
fn gain_resources_active_target_rejects_without_active_investigator() {
    // No active investigator (default phase is Mythos), so
    // InvestigatorTarget::Active should fail to resolve.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::Active, 1),
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
}
