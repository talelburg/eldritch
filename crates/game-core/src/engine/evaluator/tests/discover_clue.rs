use super::*;

#[test]
fn discover_clue_moves_one_clue_from_location_to_controller() {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(loc_id);
    let mut location = test_support::test_location(10, "Study");
    location.clues = 3;

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
        &dsl::discover_clue(LocationTarget::YourLocation, 1),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.locations[&loc_id].clues, 2);
    assert_eq!(state.investigators[&inv_id].clues, 1);
    assert_event!(
        events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        events,
        Event::LocationCluesChanged { location, new_count: 2 } if *location == loc_id
    );
}

#[test]
fn discover_clue_without_registry_discovers_normally() {
    // No registry installed (game-core unit context) → the interrupt
    // scan finds nothing → discovery proceeds exactly as before.
    // Regression guard for the seam's "fall through" path (C5a #236).
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(loc_id);
    let mut location = test_support::test_location(10, "Study");
    location.clues = 3;

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
        &dsl::discover_clue(LocationTarget::YourLocation, 1),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(
        state.open_windows().is_empty(),
        "no before-discover window opens without a registry"
    );
    assert_eq!(state.locations[&loc_id].clues, 2);
    assert_eq!(state.investigators[&inv_id].clues, 1);
}

#[test]
fn discover_clue_caps_at_location_clue_count() {
    // Card asks for 3 clues but the location only has 1 — take
    // what's there, no error.
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
        &dsl::discover_clue(LocationTarget::YourLocation, 3),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.locations[&loc_id].clues, 0);
    assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
    assert_event!(
        events,
        Event::CluePlaced {
            investigator: _,
            count: 1
        }
    );
}

#[test]
fn discover_clue_on_empty_location_is_a_silent_noop() {
    // Per the rulebook: a discover-clue effect against an empty
    // location is a no-op, not a rejection.
    let loc_id = LocationId(10);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(loc_id);
    let location = test_support::test_location(10, "Study"); // 0 clues by default

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
        &dsl::discover_clue(LocationTarget::YourLocation, 1),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.locations[&loc_id].clues, 0);
    assert_eq!(state.investigators[&InvestigatorId(1)].clues, 0);
    assert_no_event!(events, Event::CluePlaced { .. });
}

#[test]
fn discover_clue_rejects_when_controller_is_between_locations() {
    // "You" has no current_location — LocationTarget::
    // YourLocation can't resolve.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1)) // current_location = None
        .build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(LocationTarget::YourLocation, 1),
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(events.is_empty());
}

#[test]
fn discover_clue_tested_location_resolves_to_in_flight_test_location() {
    // LocationTarget::TestedLocation reads
    // GameState::in_flight_skill_test.tested_location, regardless
    // of where the controller currently is. Set the controller's
    // current_location to a *different* location and confirm the
    // discover lands at the tested location.
    let tested = LocationId(20);
    let elsewhere = LocationId(30);
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(elsewhere);
    let mut tested_loc = test_support::test_location(20, "Study");
    tested_loc.clues = 2;
    let elsewhere_loc = test_support::test_location(30, "Hall");

    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_location(tested_loc)
        .with_location(elsewhere_loc)
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id: SkillTestId(0),
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            kind: SkillTestKind::Investigate,
            difficulty_basis: DifficultyBasis::Fixed(2),
            committed_by_active: Vec::new(),
            tested_location: Some(tested),
            follow_up: SkillTestFollowUp::Investigate,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: SkillTestStep::AwaitingCommit,
            bonus_attack_damage: 0,
            bonus_clues_discovered: 0,
            resolved: None,
            symbol_on_fail: None,
        }));
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(LocationTarget::TestedLocation, 1),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.locations[&tested].clues, 1);
    assert_eq!(state.locations[&elsewhere].clues, 0);
    assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
}

#[test]
fn tested_location_rejects_without_in_flight_test() {
    // No in-flight skill test → TestedLocation can't resolve.
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(LocationId(10));
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_location({
            let mut l = test_support::test_location(10, "Study");
            l.clues = 1;
            l
        })
        .build();
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(LocationTarget::TestedLocation, 1),
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(events.is_empty());
}

#[test]
fn tested_location_rejects_when_test_has_no_location_snapshot() {
    // In-flight test exists but tested_location is None (e.g.
    // a bare plain skill test invoked while between locations).
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            id: SkillTestId(0),
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            kind: SkillTestKind::Plain,
            difficulty_basis: DifficultyBasis::Fixed(2),
            committed_by_active: Vec::new(),
            tested_location: None,
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
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(LocationTarget::TestedLocation, 1),
        ctx(1),
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(events.is_empty());
}
