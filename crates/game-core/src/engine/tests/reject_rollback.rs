use super::*;
use crate::test_support::TEST_REGISTRIES;

// The subject of this file is the rejection of a `ResolveInput` itself, so
// each test sends one raw. The first two send it with no prompt outstanding:
// that empty stack is the moment under test, so the state is applied to
// directly rather than settled into a session.

#[test]
fn rejected_actions_do_not_mutate_state() {
    let state = GameStateBuilder::new().build();
    let round_before = state.round;
    let phase_before = state.phase;
    let active_before = state.active_investigator;

    // A ResolveInput with no outstanding prompt is Rejected.
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
        TEST_REGISTRIES,
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Investigation)
        .with_seat(seat(1))
        .with_active_investigator(INV)
        .build();
    let before = state.clone();

    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
        TEST_REGISTRIES,
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
    let paused = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_chaos_bag(bag_only_zero())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 2);
    assert_eq!(
        paused.prompt().kind,
        InputKind::PickMultiple,
        "skill test should pause at the commit window",
    );
    assert!(paused.state().has_skill_test_in_flight());
    let s1 = paused.state().clone();

    // Malformed response: commit window expects PickMultiple; send Skip.
    let rejected = paused.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::Skip,
    }));
    rejected.expect_rejected();
    let result = rejected.finish();

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

#[test]
fn engine_record_applied_while_a_prompt_is_outstanding_is_rejected() {
    // In a replay log an engine record only ever sits where the engine was
    // at rest, never between a prompt and its answer. Applied mid-prompt it
    // would run beneath the outstanding prompt and strand it, so it is
    // rejected under the validate-first contract.
    let roster = [RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: (0..20).map(|i| CardCode::new(format!("d-{i}"))).collect(),
        random_basic_weaknesses: 0,
    }];
    let opened = seat_and_open(
        GameStateBuilder::new().with_rng_seed(7).build(),
        &roster,
        0,
        TEST_REGISTRIES,
    );
    assert!(
        matches!(opened.outcome, EngineOutcome::AwaitingInput { .. }),
        "seating should pause on the setup mulligan, got {:?}",
        opened.outcome
    );
    let before = opened.state.clone();

    let result = apply(
        opened.state,
        Action::Engine(EngineRecord::DeckShuffled {
            investigator: InvestigatorId(1),
        }),
        TEST_REGISTRIES,
    );

    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("expected Rejected, got {:?}", result.outcome);
    };
    assert!(
        reason.contains("prompt is outstanding"),
        "the reason should name the outstanding prompt: {reason}"
    );
    assert_eq!(
        result.state, before,
        "rejected record must not mutate state"
    );
    assert!(result.events.is_empty());
}
