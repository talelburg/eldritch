use super::*;

/// The [`star_board_from`] with investigator 1 seated in the Hallway and
/// `enemies` on it.
fn star_board_with_investigator_in_hallway(
    enemies: impl IntoIterator<Item = StagedEnemy<Printed>>,
) -> GameState {
    let mut builder = GameStateBuilder::new()
        .with_registries(crate::TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HALL))
        .with_seating([InvestigatorId(1)]);
    for enemy in enemies {
        builder = builder.with_enemy(enemy);
    }
    star_board_from(builder)
}

/// The [`star_board_from`] with a Barricade at each of `barricaded` and `ghoul` on
/// it.
fn star_board_with(ghoul: StagedEnemy<Printed>, barricaded: &[LocationId]) -> GameState {
    star_board_barricading(
        GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_enemy(ghoul),
        barricaded,
    )
}

/// Regression for #811: no Parlor on the board is a **no-op**, not a
/// rejection: `Done`, no events, nothing moved. The reachable case is
/// agenda 3 turning up while the act deck is still on act 1 or 2 — see the
/// function's own doc for why. `run_done` already asserts the `Done`.
#[test]
fn no_parlor_in_play_moves_nothing_and_does_not_reject() {
    // The star map without its Parlor.
    let mut state = GameStateBuilder::new()
        .with_registries(crate::TEST_REGISTRIES)
        .with_location(gathering_location(HALL, "01112", &[ATTIC, CELLAR], false))
        .with_location(gathering_location(ATTIC, "01113", &[HALL], false))
        .with_location(gathering_location(CELLAR, "01114", &[HALL], false))
        .with_enemy(ghoul(1, ATTIC))
        .build();
    let events = run_done(&mut state, move_ghouls_toward_parlor);
    assert!(events.is_empty(), "no events: {events:?}");
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(ATTIC),
        "the Ghoul stays put — there is no destination to step toward",
    );
}

#[test]
fn unengaged_ghoul_in_attic_steps_to_hallway() {
    let mut state = star_board_with(ghoul(1, ATTIC), &[]);
    let events = run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(HALL));
    assert!(events.iter().any(|e| matches!(e,
        Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == HALL)));
}

#[test]
fn ghoul_in_hallway_steps_to_parlor() {
    let mut state = star_board_with(ghoul(1, HALL), &[]);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(PARLOR));
}

#[test]
fn non_elite_ghoul_does_not_move_into_a_barricaded_parlor() {
    // A Barricade (01038) on the Parlor blocks the non-Elite Ghoul's
    // compelled step, so it does not move. Needs the real registry so the
    // attachment's `EnemyMovementBlocked` restriction is read.
    let mut state = star_board_with(ghoul(1, HALL), &[PARLOR]);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(HALL),
        "Ghoul stayed in the Hallway — the only step toward the Parlor is blocked",
    );
}

/// A detour board: Attic -> Hallway -> Parlor is the shortest route
/// (2 steps); Attic -> Study -> Cellar -> Parlor is the long way (3).
/// Synthetic topology on real Gathering codes — the printed map is the
/// star in [`star_board_from`], which has no detour to test against. A Barricade
/// sits at each of `barricaded`, and `ghoul` is on the board.
fn detour_board(ghoul: StagedEnemy<Printed>, barricaded: &[LocationId]) -> GameState {
    let loc = |id, code, connections: &[LocationId]| {
        gathering_location(id, code, connections, barricaded.contains(&id))
    };
    GameStateBuilder::new()
        .with_registries(crate::TEST_REGISTRIES)
        .with_location(loc(HALL, "01112", &[ATTIC, PARLOR]))
        .with_location(loc(ATTIC, "01113", &[HALL, STUDY]))
        .with_location(loc(CELLAR, "01114", &[STUDY, PARLOR]))
        .with_location(loc(PARLOR, "01115", &[HALL, CELLAR]))
        .with_location(loc(STUDY, "01111", &[ATTIC, CELLAR]))
        .with_enemy(ghoul)
        .build()
}

/// Regression for #797: a barricaded compelled step is a non-move, not a
/// detour. `glossary/Patrol.md`: *"If an enemy with patrol would be
/// compelled to move to a location which is blocked by a card ability, the
/// enemy does not move."* 01107 is structurally the same fixed-destination,
/// shortest-path mover.
#[test]
fn ghoul_does_not_reroute_around_a_barricaded_shortest_step() {
    // The Hallway is the only shortest step.
    let mut state = detour_board(ghoul(1, ATTIC), &[HALL]);

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(ATTIC),
        "the Ghoul stays put rather than taking the long way through the Study",
    );
    assert!(!events.iter().any(|e| matches!(e, Event::EnemyMoved { .. })));
}

#[test]
fn ghoul_takes_the_shortest_step_when_nothing_is_barricaded() {
    let mut state = detour_board(ghoul(1, ATTIC), &[]);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(HALL));
}

#[test]
fn a_barricade_off_the_shortest_path_does_not_affect_the_move() {
    // The Study is on the long route only.
    let mut state = detour_board(ghoul(1, ATTIC), &[STUDY]);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(HALL));
}

/// Regression for #633: a Ghoul that steps into a lone investigator's
/// location engages on arrival. `Enemy_Engagement.md`: *"Any time a ready unengaged
/// enemy is at the same location as an investigator, it engages that
/// investigator"*, listed example *"It moves into the same location as
/// an investigator"*.
#[test]
fn ghoul_moving_into_the_investigators_location_engages_on_arrival() {
    let mut state = star_board_with_investigator_in_hallway([ghoul(1, ATTIC)]);

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(HALL),
        "Ghoul stepped Attic -> Hallway"
    );
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1)),
        "and engaged the investigator standing there"
    );
    assert!(events.iter().any(|e| matches!(e,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == InvestigatorId(1))));
}

/// *"This agenda can move exhausted (evaded) enemies"*
/// (<https://arkhamdb.com/card/01107>), but `Enemy_Engagement.md`:
/// *"An exhausted unengaged enemy does not engage"* — so it arrives
/// unengaged and engages only when it readies.
#[test]
fn exhausted_ghoul_moved_into_the_investigators_location_arrives_unengaged() {
    let mut state = star_board_with_investigator_in_hallway([ghoul(1, ATTIC).exhausted()]);

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(HALL),
        "the evaded Ghoul still moves"
    );
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        None,
        "but an exhausted unengaged enemy does not engage"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::EnemyEngaged { .. })));
}

#[test]
fn engaged_ghoul_and_ghoul_at_parlor_do_not_move() {
    let mut state = star_board_from(
        GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_seat(test_support::seat(1).at(ATTIC))
            .with_enemy(ghoul(1, ATTIC).engaged_with(InvestigatorId(1)))
            .with_enemy(ghoul(2, PARLOR)), // already at Parlor
    );
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(ATTIC));
    assert_eq!(state.enemies[&EnemyId(2)].current_location, Some(PARLOR));
}

#[test]
fn non_ghoul_does_not_move() {
    // Swarm of Rats 01159 (Creature.): traits, but not Ghoul.
    let mut state = star_board_with(test_support::enemy(1).as_card("01159").at(ATTIC), &[]);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(ATTIC));
}
