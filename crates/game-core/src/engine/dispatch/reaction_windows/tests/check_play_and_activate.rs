use super::*;
use crate::state::{CardInstanceId, GameStateBuilder};
use crate::test_support;

#[test]
fn check_play_card_returns_err_for_unknown_hand_index() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let err = check_play_card(&state, InvestigatorId(1), 0).expect_err("empty hand should reject");
    assert!(
        err.contains("hand_index"),
        "error should mention hand_index, got: {err}"
    );
}

#[test]
fn check_play_card_returns_err_when_investigator_missing() {
    let state = GameStateBuilder::default().build();
    let err = check_play_card(&state, InvestigatorId(99), 0)
        .expect_err("missing investigator should reject");
    assert!(
        err.contains("not in state"),
        "error should say not in state, got: {err}"
    );
}

#[test]
fn check_activate_ability_returns_err_for_unreachable_source() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let err = check_activate_ability(
        &state,
        InvestigatorId(1),
        AbilitySource::InPlay(CardInstanceId(999)),
        &AbilityAddress::Printed(0),
    )
    .expect_err("a source the investigator cannot reach should reject");
    assert!(
        err.contains("cannot reach"),
        "error should say the source is unreachable, got: {err}"
    );
}

#[test]
fn check_activate_ability_returns_err_when_investigator_missing() {
    let state = GameStateBuilder::default().build();
    let err = check_activate_ability(
        &state,
        InvestigatorId(99),
        AbilitySource::InPlay(CardInstanceId(1)),
        &AbilityAddress::Printed(0),
    )
    .expect_err("missing investigator should reject");
    assert!(
        err.contains("not in state"),
        "error should say not in state, got: {err}"
    );
}
