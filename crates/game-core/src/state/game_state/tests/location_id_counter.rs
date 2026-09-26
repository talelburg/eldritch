use crate::state::{Counter, GameState, GameStateBuilder};

#[test]
fn game_state_starts_location_ids_at_zero() {
    let state = GameStateBuilder::new().build();
    assert_eq!(state.location_ids.peek(), 0);
}

#[test]
fn location_ids_round_trip_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.location_ids = Counter::at(7);
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.location_ids.peek(), 7);
}
