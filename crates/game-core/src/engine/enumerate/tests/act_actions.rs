use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

/// An open-turn state with an advanceable act (threshold `t`) and the
/// investigator holding `clues`.
fn open_turn_with_act(threshold: u8, clues: u8) -> GameState {
    GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(clues))
        .open_turn(InvestigatorId(1))
        .with_act_deck([act("_test_act", threshold)])
        .build()
}

#[test]
fn advance_act_offered_when_clues_meet_threshold() {
    let state = open_turn_with_act(2, 2);
    assert!(
        legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        })
    );
}

#[test]
fn advance_act_absent_when_clues_insufficient() {
    let state = open_turn_with_act(2, 1);
    assert!(
        !legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        })
    );
}

#[test]
fn advance_act_absent_with_no_act_deck() {
    // A bare open turn has an empty act_deck → AdvanceAct not offered.
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .open_turn(InvestigatorId(1))
        .build();
    assert!(
        !legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::AdvanceAct {
            investigator: InvestigatorId(1),
        })
    );
}
