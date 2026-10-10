use super::*;

#[test]
fn investigate_succeeds_and_moves_one_clue_to_investigator() {
    // Default intellect 3, shroud 2 → margin 1 → success.
    let (inv_id, loc_id, state) = investigate_scenario(2, 2);
    let result = take_action_no_commits(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::SkillTestStarted {
            skill: SkillKind::Intellect,
            difficulty: 2,
            ..
        }
    );
    assert_event!(result.events, Event::SkillTestSucceeded { margin: 1, .. });
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::LocationCluesChanged { location, new_count: 1 } if *location == loc_id
    );
    assert_eq!(result.state.investigators[&inv_id].clues, 1);
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn investigate_failure_spends_action_but_moves_no_clue() {
    // Intellect 3, shroud 5 → fails by 2; action still spent.
    let (inv_id, loc_id, state) = investigate_scenario(2, 5);
    let result = take_action_no_commits(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestFailed { by: 2, .. });
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_no_event!(result.events, Event::LocationCluesChanged { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
    assert_eq!(result.state.investigators[&inv_id].clues, 0);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn investigate_at_empty_location_spends_action_and_runs_test_silently() {
    // Location has 0 clues; the test still fires (you can't tell
    // the location is empty without trying), the action is still
    // spent, and discover_clue is a silent no-op on success.
    let (inv_id, loc_id, state) = investigate_scenario(0, 2);
    let result = take_action_no_commits(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    assert_eq!(result.state.locations[&loc_id].clues, 0);
    assert_eq!(result.state.investigators[&inv_id].clues, 0);
}

#[test]
fn investigate_outside_investigation_phase_is_rejected() {
    let (inv_id, _, mut state) = investigate_scenario(2, 2);
    state.phase = Phase::Mythos;
    // Mythos phase → Investigate is not a legal open-turn action.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == inv_id)));
}

#[test]
fn investigate_by_non_active_investigator_is_rejected() {
    let (_, _, mut state) = investigate_scenario(2, 2);
    // Add a second investigator but keep the first active.
    let other = InvestigatorId(2);
    state
        .investigators
        .insert(other, test_support::test_investigator(2));
    // Non-active investigator → their Investigate is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == other)));
}

#[test]
fn investigate_with_zero_actions_is_rejected() {
    let (inv_id, _, mut state) = investigate_scenario(2, 2);
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .actions_remaining = 0;
    // No actions remaining → Investigate is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == inv_id)));
}

#[test]
fn investigate_without_a_current_location_is_rejected() {
    let (inv_id, _, mut state) = investigate_scenario(2, 2);
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .current_location = None;
    // No current location → Investigate is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == inv_id)));
}

#[test]
#[should_panic(expected = "state-corruption invariant violation")]
fn investigate_with_dangling_current_location_panics() {
    // Corruption case: investigator's current_location references
    // a location not in state.locations. Matches the loud-on-
    // corruption pattern used by end_turn / rotate_to_active.
    let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
    state.locations.remove(&loc_id);
    let _ = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );
}

#[test]
fn investigate_on_an_unrevealed_location_is_rejected() {
    // An unrevealed location is not yet investigatable (Rules Reference
    // p.14). In practice unreachable (entering reveals), but the gate
    // makes the rule explicit.
    let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
    state.locations.get_mut(&loc_id).unwrap().revealed = false;
    // Unrevealed location → Investigate is not legal.
    assert!(
        !legal_actions(&state).iter().any(
            |a| matches!(a, TurnAction::Investigate { investigator } if *investigator == inv_id)
        ),
        "unrevealed location cannot be investigated"
    );
}

#[test]
fn investigate_with_active_investigator_missing_from_map_rejects() {
    // Same case as `move_with_active_investigator_missing_from_map_rejects`
    // (`move_action.rs`), applied to Investigate.
    let (inv_id, _, mut state) = investigate_scenario(2, 2);
    state.investigators.remove(&inv_id);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
}
