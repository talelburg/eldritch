use super::*;

#[test]
fn act_and_agenda_carry_card_code() {
    let act = Act {
        code: CardCode("01108".into()),
        clue_threshold: 2,
    };
    let agenda = Agenda {
        code: CardCode("01105".into()),
        doom_threshold: 3,
    };
    assert_eq!(act.code, CardCode("01108".into()));
    assert_eq!(agenda.code, CardCode("01105".into()));
}

#[test]
fn game_state_is_partial_eq() {
    fn assert_partial_eq<T: PartialEq>() {}
    assert_partial_eq::<GameState>();
}

#[test]
fn saved_state_whose_enemy_carries_traits_still_loads() {
    // A save may carry a `traits` key on each enemy, a copy of its printed
    // traits that the engine does not keep. It must keep loading: the key is
    // ignored, not rejected.
    let enemy = crate::test_support::test_enemy(1, "Ghoul Minion");
    let state = crate::state::GameStateBuilder::new()
        .with_enemy(enemy)
        .build();
    let mut json = serde_json::to_value(&state).expect("serialize");
    json["enemies"]
        .as_object_mut()
        .expect("enemies serialize to a JSON object")
        .values_mut()
        .next()
        .expect("one enemy in the save")
        .as_object_mut()
        .expect("an enemy serializes to a JSON object")
        .insert(
            "traits".to_owned(),
            serde_json::json!(["Humanoid", "Monster", "Ghoul"]),
        );

    let back: GameState = serde_json::from_value(json).expect("an old save still deserializes");
    assert_eq!(back.enemies, state.enemies);
}
