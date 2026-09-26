use crate::state::GameStateBuilder;

#[test]
fn game_state_default_has_no_encounter_draw_pending() {
    let state = GameStateBuilder::new().build();
    assert_eq!(state.current_encounter_drawer(), None);
}
