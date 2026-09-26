use super::*;

#[test]
fn rejected_actions_do_not_mutate_state() {
    let state = GameStateBuilder::new().build();
    let round_before = state.round;
    let phase_before = state.phase;
    let active_before = state.active_investigator;

    // ResolveInput is a Phase-1 stub — guaranteed to be Rejected.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, _);
    assert_eq!(result.state.round, round_before);
    assert_eq!(result.state.phase, phase_before);
    assert_eq!(result.state.active_investigator, active_before);
}

#[test]
fn guard_ladder_reject_leaves_state_byte_identical() {
    // A guard-ladder reject (ResolveInput against a state with no
    // in-flight skill test, no open windows, and no pending hunter
    // move) fires *before any mutation*. This locks that pre-mutation
    // rejects return the input state byte-identical (whole-state
    // equality, stronger than the field-by-field
    // `rejected_actions_do_not_mutate_state` above). The mid-resolution
    // rollback path — where a handler mutates *then* rejects — is
    // covered by `rejected_resolve_input_rewinds_to_pause_state_not_pre_action`
    // and the integration test in `crates/cards/tests/reject_rollback.rs`.
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let before = state.clone();

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state, before,
        "rejected action must not mutate state"
    );
    assert!(result.events.is_empty());
}

#[test]
fn rejected_resolve_input_rewinds_to_pause_state_not_pre_action() {
    // Drive a skill test to its commit-window AwaitingInput, then submit
    // a malformed response. The reject must rewind to the *pause* state
    // (in_flight_skill_test still set), not to before the skill test.
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_chaos_bag(bag_only_zero())
        .build();

    let paused =
        test_support::perform_skill_test(state, InvestigatorId(1), SkillKind::Willpower, 2);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "skill test should pause at the commit window, got {:?}",
        paused.outcome,
    );
    assert!(paused.state.has_skill_test_in_flight());
    let s1 = paused.state.clone();

    // Malformed response: commit window expects PickMultiple; send Skip.
    let result = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state, s1,
        "rejected ResolveInput rewinds to the pause state, not pre-action",
    );
    assert!(
        result.state.has_skill_test_in_flight(),
        "suspension stays open"
    );
    assert!(result.events.is_empty());
}
