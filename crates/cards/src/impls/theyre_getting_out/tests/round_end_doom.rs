use super::*;

/// The [`star_board_from`] on top of `builder`, on agenda 01107 itself.
fn star_board_on_this_agenda(builder: GameStateBuilder) -> GameState {
    star_board_from(
        builder
            .with_registries(crate::TEST_REGISTRIES)
            .with_agenda_deck(["01107"]),
    )
}

#[test]
fn doom_counts_ghouls_in_hallway_and_parlor_only() {
    let mut state = star_board_on_this_agenda(
        GameStateBuilder::new()
            .with_enemy(ghoul(1, HALL)) // counts
            .with_enemy(ghoul(2, PARLOR)) // counts
            .with_enemy(ghoul(3, ATTIC)) // no
            // Swarm of Rats 01159 (Creature.): traits, but not Ghoul. In the
            // Hallway, it doesn't count.
            .with_enemy(test_support::enemy(4).as_card("01159").at(HALL)),
    );
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 2);
}

#[test]
fn engaged_ghoul_in_hallway_still_counts_for_doom() {
    let mut state = star_board_on_this_agenda(
        GameStateBuilder::new()
            .with_seat(test_support::seat(1).at(HALL))
            .with_enemy(ghoul(1, HALL).engaged_with(InvestigatorId(1))),
    );
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 1);
}

/// The empty-agenda-deck guard: with no agenda to place doom on, a Ghoul
/// in the Hallway places none.
#[test]
fn round_end_doom_on_an_empty_agenda_deck_is_a_no_op() {
    let mut state = star_board_from(
        GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_enemy(ghoul(1, HALL)),
    );
    assert!(state.agenda_deck.is_empty());
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 0);
}
