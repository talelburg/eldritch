use super::*;

#[test]
fn game_state_is_partial_eq() {
    fn assert_partial_eq<T: PartialEq>() {}
    assert_partial_eq::<GameState>();
}
