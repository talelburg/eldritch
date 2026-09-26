use super::*;

/// An open-turn state with an advanceable act (threshold `t`) and the
/// investigator holding `clues`.
fn open_turn_with_act(threshold: u8, clues: u8) -> GameState {
    let mut state = open_turn_state();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .clues = clues;
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: threshold,
    }];
    state
}

#[test]
fn advance_act_offered_when_clues_meet_threshold() {
    let state = open_turn_with_act(2, 2);
    assert!(legal_actions(&state).contains(&TurnAction::AdvanceAct {
        investigator: InvestigatorId(1),
    }));
}

#[test]
fn advance_act_absent_when_clues_insufficient() {
    let state = open_turn_with_act(2, 1);
    assert!(!legal_actions(&state).contains(&TurnAction::AdvanceAct {
        investigator: InvestigatorId(1),
    }));
}

#[test]
fn advance_act_absent_with_no_act_deck() {
    // open_turn_state has an empty act_deck → AdvanceAct not offered.
    let state = open_turn_state();
    assert!(!legal_actions(&state).contains(&TurnAction::AdvanceAct {
        investigator: InvestigatorId(1),
    }));
}
