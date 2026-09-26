use super::*;
use crate::state::{Counter, GameState, GameStateBuilder};

#[test]
fn game_state_starts_enemy_ids_at_zero() {
    let state = GameStateBuilder::new().build();
    assert_eq!(state.enemy_ids.peek(), 0);
}

#[test]
fn enemy_ids_round_trip_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.enemy_ids = Counter::at(42);
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.enemy_ids.peek(), 42);
}

#[test]
fn each_id_counter_mints_independently() {
    let mut state = GameStateBuilder::new().build();
    assert_eq!(state.card_instance_ids.mint(), CardInstanceId(0));
    assert_eq!(state.card_instance_ids.mint(), CardInstanceId(1));
    // Each id type draws from its own counter — minting one doesn't
    // disturb the others.
    assert_eq!(state.enemy_ids.mint(), EnemyId(0));
    assert_eq!(state.location_ids.mint(), LocationId(0));
    assert_eq!(state.enemy_ids.mint(), EnemyId(1));
    assert_eq!(state.card_instance_ids.peek(), 2);
    assert_eq!(state.enemy_ids.peek(), 2);
    assert_eq!(state.location_ids.peek(), 1);
}

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
