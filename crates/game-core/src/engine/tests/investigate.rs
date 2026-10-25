use super::*;
use crate::test_support::TEST_REGISTRY;

const INVESTIGATE: TurnAction = TurnAction::Investigate { investigator: INV };

/// Whether `state`'s menu offers `investigator` an Investigate.
fn offers_investigate(state: &GameState, investigator: InvestigatorId) -> bool {
    legal_actions(state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator: i } if *i == investigator))
}

#[test]
fn investigate_succeeds_and_moves_one_clue_to_investigator() {
    // Default intellect 3, shroud 2 → margin 1 → success.
    let session = take_committing_nothing(investigate_board(seat(1).at(HERE), 2, 2), &INVESTIGATE);

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == INV
    );
    assert_event!(
        events,
        Event::SkillTestStarted {
            skill: SkillKind::Intellect,
            difficulty: 2,
            ..
        }
    );
    assert_event!(events, Event::SkillTestSucceeded { margin: 1, .. });
    assert_event!(
        events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_event!(
        events,
        Event::LocationCluesChanged { location, new_count: 1 } if *location == HERE
    );
    let state = session.state();
    assert_eq!(state.investigators[&INV].clues, 1);
    assert_eq!(state.locations[&HERE].clues, 1);
    assert_eq!(state.investigators[&INV].actions_remaining, 2);
}

#[test]
fn investigate_failure_spends_action_but_moves_no_clue() {
    // Intellect 3, shroud 5 → fails by 2; action still spent.
    let session = take_committing_nothing(investigate_board(seat(1).at(HERE), 2, 5), &INVESTIGATE);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { by: 2, .. });
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_no_event!(session.events(), Event::LocationCluesChanged { .. });
    let state = session.state();
    assert_eq!(state.locations[&HERE].clues, 2);
    assert_eq!(state.investigators[&INV].clues, 0);
    assert_eq!(state.investigators[&INV].actions_remaining, 2);
}

#[test]
fn investigate_at_empty_location_spends_action_and_runs_test_silently() {
    // Location has 0 clues; the test still fires (you can't tell
    // the location is empty without trying), the action is still
    // spent, and discover_clue is a silent no-op on success.
    let session = take_committing_nothing(investigate_board(seat(1).at(HERE), 0, 2), &INVESTIGATE);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    let state = session.state();
    assert_eq!(state.investigators[&INV].actions_remaining, 2);
    assert_eq!(state.locations[&HERE].clues, 0);
    assert_eq!(state.investigators[&INV].clues, 0);
}

#[test]
fn investigate_outside_investigation_phase_is_rejected() {
    let state = investigate_board(seat(1).at(HERE), 2, 2)
        .with_phase(Phase::Mythos)
        .build();
    // Mythos phase → Investigate is not a legal open-turn action.
    assert!(!offers_investigate(&state, INV));
}

#[test]
fn investigate_by_non_active_investigator_is_rejected() {
    // Add a second investigator but keep the first active.
    let other = InvestigatorId(2);
    let state = investigate_board(seat(1).at(HERE), 2, 2)
        .with_seat(seat(2))
        .build();
    // Non-active investigator → their Investigate is not legal.
    assert!(!offers_investigate(&state, other));
}

#[test]
fn investigate_with_zero_actions_is_rejected() {
    let state = investigate_board(seat(1).at(HERE).actions(0), 2, 2).build();
    // No actions remaining → Investigate is not legal.
    assert!(!offers_investigate(&state, INV));
}

#[test]
fn investigate_without_a_current_location_is_rejected() {
    let state = investigate_board(seat(1), 2, 2).build();
    // No current location → Investigate is not legal.
    assert!(!offers_investigate(&state, INV));
}

#[test]
#[should_panic(expected = "state-corruption invariant violation")]
fn investigate_with_dangling_current_location_panics() {
    // Corruption case: investigator's current_location references
    // a location not in state.locations. Matches the loud-on-
    // corruption pattern used by end_turn / rotate_to_active. The
    // corruption is made at rest, without re-settling, so it reaches the
    // handler rather than the menu.
    drop(
        investigate_board(seat(1).at(HERE), 2, 2)
            .session()
            .edit_state(|state| {
                state.locations.remove(&HERE);
            })
            .submit_off_menu(&INVESTIGATE),
    );
}

#[test]
fn investigate_on_an_unrevealed_location_is_rejected() {
    // An unrevealed location is not yet investigatable (Rules Reference
    // p.14). In practice unreachable (entering reveals), but the gate
    // makes the rule explicit.
    let mut state = investigate_board(seat(1).at(HERE), 2, 2).build();
    state.locations.get_mut(&HERE).unwrap().revealed = false;
    // Unrevealed location → Investigate is not legal.
    assert!(
        !offers_investigate(&state, INV),
        "unrevealed location cannot be investigated"
    );
}

#[test]
fn investigate_with_active_investigator_missing_from_map_rejects() {
    // Same case as `move_with_active_investigator_missing_from_map_rejects`
    // (`move_action.rs`), applied to Investigate. The investigator is
    // removed at rest, without re-settling, so the action reaches the
    // handler rather than the menu.
    let session = investigate_board(seat(1).at(HERE), 2, 2)
        .session()
        .edit_state(|state| {
            state.investigators.remove(&INV);
        })
        .submit_off_menu(&INVESTIGATE);
    session.expect_rejected();
}
