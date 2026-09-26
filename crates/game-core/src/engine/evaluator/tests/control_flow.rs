use super::*;

fn state_with_in_flight_kind(kind: SkillTestKind) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator({
            let mut inv = test_support::test_investigator(1);
            inv.current_location = Some(LocationId(10));
            inv
        })
        .with_location({
            let mut l = test_support::test_location(10, "Study");
            l.clues = 2;
            l
        })
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id: SkillTestId(0),
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            kind,
            difficulty_basis: DifficultyBasis::Fixed(2),
            committed_by_active: Vec::new(),
            tested_location: Some(LocationId(10)),
            follow_up: SkillTestFollowUp::None,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: SkillTestStep::AwaitingCommit,
            bonus_attack_damage: 0,
            bonus_clues_discovered: 0,
            resolved: None,
            symbol_on_fail: None,
        }));
    state
}

#[test]
fn if_skill_test_kind_runs_then_branch_when_kind_matches() {
    let mut state = state_with_in_flight_kind(SkillTestKind::Investigate);
    let mut events = Vec::new();
    let effect = dsl::if_(
        Condition::SkillTestKind(SkillTestKind::Investigate),
        dsl::discover_clue(LocationTarget::TestedLocation, 1),
    );

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.locations[&LocationId(10)].clues, 1);
    assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
}

#[test]
fn if_skill_test_kind_skips_then_branch_when_kind_differs() {
    let mut state = state_with_in_flight_kind(SkillTestKind::Plain);
    let mut events = Vec::new();
    let effect = dsl::if_(
        Condition::SkillTestKind(SkillTestKind::Investigate),
        dsl::discover_clue(LocationTarget::TestedLocation, 1),
    );

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    // No-op: location clues unchanged, no events emitted.
    assert_eq!(state.locations[&LocationId(10)].clues, 2);
    assert_eq!(state.investigators[&InvestigatorId(1)].clues, 0);
    assert!(events.is_empty());
}

#[test]
fn if_skill_test_kind_runs_else_branch_when_present_and_kind_differs() {
    let mut state = state_with_in_flight_kind(SkillTestKind::Fight);
    let mut events = Vec::new();
    let effect = dsl::if_else(
        Condition::SkillTestKind(SkillTestKind::Investigate),
        dsl::discover_clue(LocationTarget::TestedLocation, 1),
        dsl::gain_resources(InvestigatorTarget::You, 2),
    );
    let resources_before = state.investigators[&InvestigatorId(1)].resources;

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    // Else branch ran: location untouched, resources +2.
    assert_eq!(state.locations[&LocationId(10)].clues, 2);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].resources,
        resources_before + 2,
    );
}

#[test]
fn if_skill_test_kind_rejects_without_in_flight_test() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let effect = dsl::if_(
        Condition::SkillTestKind(SkillTestKind::Investigate),
        dsl::discover_clue(LocationTarget::TestedLocation, 1),
    );

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(events.is_empty());
}

#[test]
fn if_skill_test_outcome_condition_remains_todo() {
    // `Condition::SkillTest { outcome }` isn't yet wired. The
    // preferred path for resolution-time outcome-gated effects is
    // Trigger::OnSkillTestResolution; the condition is reserved
    // for a future past-test reaction model.
    let mut state = state_with_in_flight_kind(SkillTestKind::Investigate);
    let mut events = Vec::new();
    let effect = dsl::if_(
        Condition::SkillTest {
            outcome: TestOutcome::Success,
        },
        dsl::discover_clue(LocationTarget::TestedLocation, 1),
    );

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );

    match outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("Condition::SkillTest"),
                "reason should mention Condition::SkillTest: {reason:?}",
            );
        }
        _ => panic!("expected Rejected for stubbed condition, got {outcome:?}"),
    }
}

#[test]
fn seq_runs_effects_in_order_then_done() {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(loc_id);
    let mut location = test_support::test_location(10, "Study");
    location.clues = 1;

    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_location(location)
        .build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::seq([
            dsl::gain_resources(InvestigatorTarget::You, 2),
            dsl::discover_clue(LocationTarget::YourLocation, 1),
        ]),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_event!(events, Event::ResourcesGained { .. });
    assert_event!(events, Event::CluePlaced { .. });
    assert_eq!(state.investigators[&inv_id].resources, 7); // 5 default + 2
    assert_eq!(state.investigators[&inv_id].clues, 1);
}

#[test]
fn seq_short_circuits_on_rejected() {
    // First effect rejects (Active without active_investigator);
    // second effect should not run.
    let loc_id = LocationId(10);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(loc_id);
    let mut location = test_support::test_location(10, "Study");
    location.clues = 1;

    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_location(location)
        .build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::seq([
            dsl::gain_resources(InvestigatorTarget::Active, 1), // rejects
            dsl::discover_clue(LocationTarget::YourLocation, 1), // shouldn't run
        ]),
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    // Location's clues should still be 1 — the discover_clue
    // never executed.
    assert_eq!(state.locations[&loc_id].clues, 1);
}
