use super::*;

#[test]
fn move_to_connected_location_spends_action_and_emits_events() {
    let (inv_id, a, b, state) = move_scenario();
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
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
        Event::InvestigatorMoved { investigator, from, to }
            if *investigator == inv_id && *from == a && *to == b
    );
    assert_eq!(
        result.state.investigators[&inv_id].current_location,
        Some(b)
    );
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn move_to_unconnected_location_is_rejected() {
    // Build a fresh scenario where C exists but A is not connected to C.
    let (inv_id, _, _, mut state) = move_scenario();
    let c = LocationId(12);
    state
        .locations
        .insert(c, test_support::test_location(12, "C"));
    // C is not connected from A → Move to C is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == c)));
}

#[test]
fn move_to_current_location_is_rejected() {
    let (inv_id, a, _, state) = move_scenario();
    // Current location → Move to A is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|act| matches!(act, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == a)));
}

#[test]
fn move_outside_investigation_phase_is_rejected() {
    let (inv_id, _, b, mut state) = move_scenario();
    state.phase = Phase::Mythos;
    // Mythos phase → Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == b)));
}

#[test]
fn move_by_non_active_investigator_is_rejected() {
    let (_, _, b, mut state) = move_scenario();
    let other = InvestigatorId(2);
    state
        .investigators
        .insert(other, test_support::test_investigator(2));
    // Non-active investigator → their Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == other && *destination == b)));
}

#[test]
fn move_with_zero_actions_is_rejected() {
    let (inv_id, _, b, mut state) = move_scenario();
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .actions_remaining = 0;
    // No actions remaining → Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == b)));
}

#[test]
fn move_without_current_location_is_rejected() {
    let (inv_id, _, b, mut state) = move_scenario();
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .current_location = None;
    // No current location → Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == b)));
}

#[test]
fn move_to_missing_destination_is_rejected() {
    // Connection list points to an id that's been removed from
    // state.locations — this isn't state corruption (the
    // current_location is intact), it's a malformed connection
    // graph the caller might fix; reject.
    let (inv_id, _a, b, mut state) = move_scenario();
    state.locations.remove(&b);
    // B removed from locations → Move to B is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == b)));
}

#[test]
#[should_panic(expected = "state-corruption invariant violation")]
fn move_with_dangling_current_location_panics() {
    // Corruption: current_location points at A but A isn't in
    // state.locations. Surface loudly per the project pattern.
    let (inv_id, a, b, mut state) = move_scenario();
    state.locations.remove(&a);
    let _ = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
}

#[test]
#[should_panic(expected = "state-corruption invariant violation")]
fn move_with_active_investigator_missing_from_map_panics() {
    // Corruption: active_investigator points at an id that isn't
    // in state.investigators. The active-investigator check passes
    // (Some(id) == active), so this case is only reachable from
    // corrupt state — panic to match end_turn / rotate_to_active.
    let (inv_id, _a, b, mut state) = move_scenario();
    state.investigators.remove(&inv_id);
    let _ = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
}

#[test]
fn move_to_a_location_that_is_not_in_play_is_rejected() {
    let (inv_id, a, _b, mut state) = move_scenario();
    // A set-aside location is a bare code — it has no LocationId at all
    // until it enters play, so the destination below names nothing in
    // `state.locations`.
    state.set_aside_cards.push(CardCode("setaside".into()));
    // Illegally connect the current location to it; the move must STILL be rejected (not in play).
    state
        .locations
        .get_mut(&a)
        .unwrap()
        .connections
        .push(LocationId(99));
    // A dangling connection is out of play → Move to it is not legal.
    assert!(
        !legal_actions(&state)
            .iter()
            .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == LocationId(99))),
        "a location that is not in play is not a legal destination"
    );
}

#[test]
fn moving_to_an_unrevealed_location_reveals_it_and_places_clues() {
    // 1 investigator; destination `b` is in play but unrevealed with a
    // per-investigator clue value. Entering reveals it and places clues.
    let (inv_id, _a, b, mut state) = move_scenario();
    let loc_b = state.locations.get_mut(&b).unwrap();
    loc_b.revealed = false;
    loc_b.clues = 0;
    loc_b.printed_clues = ClueValue::PerInvestigator(2);
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    let loc_b = &result.state.locations[&b];
    assert!(loc_b.revealed, "entering an unrevealed location reveals it");
    assert_eq!(loc_b.clues, 2, "1 investigator × 2 per-investigator");
}
