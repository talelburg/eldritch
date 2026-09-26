use super::*;
use crate::state::GameStateBuilder;

#[test]
fn game_state_starting_location_defaults_to_none_and_roundtrips() {
    let mut state = GameStateBuilder::new().build();
    assert_eq!(state.starting_location, None, "default must be None");

    state.starting_location = Some(LocationId(7));
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.starting_location, Some(LocationId(7)));
}
