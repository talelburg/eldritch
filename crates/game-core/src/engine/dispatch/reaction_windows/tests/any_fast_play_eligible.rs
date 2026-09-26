use super::*;
use crate::state::GameStateBuilder;
use crate::test_support;

#[test]
fn returns_false_when_no_investigators() {
    let state = GameStateBuilder::default().build();
    assert!(!any_fast_play_eligible(&state));
}

#[test]
fn returns_false_when_hands_and_in_play_empty() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .build();
    assert!(!any_fast_play_eligible(&state));
}
