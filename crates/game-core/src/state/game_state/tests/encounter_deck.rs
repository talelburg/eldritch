use super::*;
use crate::state::GameStateBuilder;

#[test]
fn encounter_deck_and_discard_serde_roundtrip() {
    let mut state = GameStateBuilder::new().build();
    state.encounter_deck.push_back(CardCode("filler1".into()));
    state.encounter_deck.push_back(CardCode("filler2".into()));
    state.encounter_discard.push(CardCode("filler99".into()));

    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.encounter_deck.len(), 2);
    assert_eq!(back.encounter_deck[0], CardCode("filler1".into()));
    assert_eq!(back.encounter_deck[1], CardCode("filler2".into()));
    assert_eq!(back.encounter_discard.len(), 1);
    assert_eq!(back.encounter_discard[0], CardCode("filler99".into()));
}

#[test]
fn fresh_state_has_empty_encounter_deck_and_discard() {
    let state = GameStateBuilder::new().build();
    assert!(state.encounter_deck.is_empty());
    assert!(state.encounter_discard.is_empty());
}

#[test]
fn game_state_default_has_no_encounter_draw_pending() {
    let state = GameStateBuilder::new().build();
    assert_eq!(state.current_encounter_drawer(), None);
}
