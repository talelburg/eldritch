use super::*;
use crate::test_support::TEST_REGISTRY;

/// Whether `state`'s menu offers `investigator` a Move to `destination`.
fn offers_move(state: &GameState, investigator: InvestigatorId, destination: LocationId) -> bool {
    legal_actions(state, &TEST_REGISTRY).iter().any(|a| {
        matches!(a, TurnAction::Move { investigator: i, destination: d } if *i == investigator && *d == destination)
    })
}

#[test]
fn move_to_connected_location_spends_action_and_emits_events() {
    let session = move_board(seat(1).at(HERE))
        .session()
        .take(&MOVE_TO_CONNECTED);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == INV
    );
    assert_event!(
        session.events(),
        Event::InvestigatorMoved { investigator, from, to }
            if *investigator == INV && *from == HERE && *to == CONNECTED
    );
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(CONNECTED)
    );
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
}

#[test]
fn move_to_unconnected_location_is_rejected() {
    // C exists but A is not connected to C.
    let c = LocationId(12);
    let state = move_board(seat(1).at(HERE))
        .with_location(test_support::test_location(c.0, "C"))
        .build();
    // C is not connected from A → Move to C is not legal.
    assert!(!offers_move(&state, INV, c));
}

#[test]
fn move_to_current_location_is_rejected() {
    let state = move_board(seat(1).at(HERE)).build();
    // Current location → Move to A is not legal.
    assert!(!offers_move(&state, INV, HERE));
}

#[test]
fn move_outside_investigation_phase_is_rejected() {
    let state = move_board(seat(1).at(HERE))
        .with_phase(Phase::Mythos)
        .build();
    // Mythos phase → Move is not legal.
    assert!(!offers_move(&state, INV, CONNECTED));
}

#[test]
fn move_by_non_active_investigator_is_rejected() {
    let other = InvestigatorId(2);
    let state = move_board(seat(1).at(HERE)).with_seat(seat(2)).build();
    // Non-active investigator → their Move is not legal.
    assert!(!offers_move(&state, other, CONNECTED));
}

#[test]
fn move_with_zero_actions_is_rejected() {
    let state = move_board(seat(1).at(HERE).actions(0)).build();
    // No actions remaining → Move is not legal.
    assert!(!offers_move(&state, INV, CONNECTED));
}

#[test]
fn move_without_current_location_is_rejected() {
    let state = move_board(seat(1)).build();
    // No current location → Move is not legal.
    assert!(!offers_move(&state, INV, CONNECTED));
}

#[test]
fn move_to_missing_destination_is_rejected() {
    // Connection list points to an id that's been removed from
    // state.locations — this isn't state corruption (the
    // current_location is intact), it's a malformed connection
    // graph the caller might fix; reject.
    let mut state = move_board(seat(1).at(HERE)).build();
    state.locations.remove(&CONNECTED);
    // B removed from locations → Move to B is not legal.
    assert!(!offers_move(&state, INV, CONNECTED));
}

#[test]
#[should_panic(expected = "state-corruption invariant violation")]
fn move_with_dangling_current_location_panics() {
    // Corruption: current_location points at A but A isn't in
    // state.locations. Surface loudly per the project pattern. The
    // corruption is made at rest, without re-settling, so it reaches the
    // handler rather than the menu.
    drop(
        move_board(seat(1).at(HERE))
            .session()
            .edit_state(|state| {
                state.locations.remove(&HERE);
            })
            .submit_off_menu(&MOVE_TO_CONNECTED),
    );
}

#[test]
fn move_with_active_investigator_missing_from_map_rejects() {
    // active_investigator names an id that isn't in state.investigators. An
    // investigator id can arrive in a client message, so taking an action
    // refuses an unknown one rather than panicking. The investigator is
    // removed at rest, without re-settling, so the action reaches the
    // handler rather than the menu.
    let session = move_board(seat(1).at(HERE))
        .session()
        .edit_state(|state| {
            state.investigators.remove(&INV);
        })
        .submit_off_menu(&MOVE_TO_CONNECTED);
    session.expect_rejected();
}

#[test]
fn move_to_a_location_that_is_not_in_play_is_rejected() {
    let mut state = move_board(seat(1).at(HERE)).build();
    // A set-aside location is a bare code — it has no LocationId at all
    // until it enters play, so the destination below names nothing in
    // `state.locations`.
    state.set_aside_cards.push(CardCode("setaside".into()));
    // Illegally connect the current location to it; the move must STILL be rejected (not in play).
    state
        .locations
        .get_mut(&HERE)
        .unwrap()
        .connections
        .push(LocationId(99));
    // A dangling connection is out of play → Move to it is not legal.
    assert!(
        !offers_move(&state, INV, LocationId(99)),
        "a location that is not in play is not a legal destination"
    );
}

#[test]
fn moving_to_an_unrevealed_location_reveals_it_and_places_clues() {
    // 1 investigator; destination `CONNECTED` is in play but unrevealed with
    // a per-investigator clue value. Entering reveals it and places clues.
    let mut destination = test_support::test_location(CONNECTED.0, "B");
    destination.revealed = false;
    destination.clues = 0;
    destination.printed_clues = ClueValue::PerInvestigator(2);
    let session = move_board(seat(1).at(HERE))
        .with_location(destination)
        .session()
        .take(&MOVE_TO_CONNECTED);
    session.assert_at_turn_menu(INV);
    let loc_b = &session.state().locations[&CONNECTED];
    assert!(loc_b.revealed, "entering an unrevealed location reveals it");
    assert_eq!(loc_b.clues, 2, "1 investigator × 2 per-investigator");
}
