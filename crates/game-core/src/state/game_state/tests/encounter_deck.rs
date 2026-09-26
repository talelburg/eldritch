use super::*;
use crate::state::GameStateBuilder;

#[test]
fn encounter_deck_and_discard_serde_roundtrip() {
    let mut state = GameStateBuilder::new().build();
    state.encounter_deck.push_back(CardCode("01001".into()));
    state.encounter_deck.push_back(CardCode("01002".into()));
    state.encounter_discard.push(CardCode("01099".into()));

    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.encounter_deck.len(), 2);
    assert_eq!(back.encounter_deck[0], CardCode("01001".into()));
    assert_eq!(back.encounter_deck[1], CardCode("01002".into()));
    assert_eq!(back.encounter_discard.len(), 1);
    assert_eq!(back.encounter_discard[0], CardCode("01099".into()));
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
