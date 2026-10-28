use super::*;

fn with_agenda(state: &mut GameState) {
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
    }];
}

#[test]
fn doom_counts_ghouls_in_hallway_and_parlor_only() {
    let mut state = star_board();
    with_agenda(&mut state);
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway — counts
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5))); // Parlor — counts
    state.enemies.insert(EnemyId(3), ghoul(3, LocationId(3))); // Attic — no

    // Swarm of Rats 01159 (Creature.): traits, but not Ghoul.
    let mut non_ghoul = test_support::test_enemy(4, "Rat");
    non_ghoul.code = CardCode::new("01159");
    non_ghoul.current_location = Some(LocationId(2));
    state.enemies.insert(EnemyId(4), non_ghoul); // Hallway non-Ghoul — no
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 2);
}

#[test]
fn engaged_ghoul_in_hallway_still_counts_for_doom() {
    let mut state = star_board();
    with_agenda(&mut state);
    let mut engaged = ghoul(1, LocationId(2)); // Hallway, engaged
    engaged.engaged_with = Some(InvestigatorId(1));
    state.enemies.insert(EnemyId(1), engaged);
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 1);
}

/// The empty-agenda-deck guard: with no agenda to place doom on, a Ghoul
/// in the Hallway places none.
#[test]
fn round_end_doom_on_an_empty_agenda_deck_is_a_no_op() {
    let mut state = star_board();
    assert!(state.agenda_deck.is_empty());
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    run_done(&mut state, place_round_end_doom);
    assert_eq!(state.agenda_doom, 0);
}
