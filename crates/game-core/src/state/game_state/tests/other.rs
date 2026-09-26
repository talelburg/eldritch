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
