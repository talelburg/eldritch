use super::*;

/// The [`star_board`] with investigator 1 seated in the Hallway.
fn star_board_with_investigator_in_hallway() -> GameState {
    star_board_from(
        GameStateBuilder::new()
            .with_registries(crate::TEST_REGISTRIES)
            .with_seat(test_support::seat(1).at(LocationId(2)))
            .with_turn_order([InvestigatorId(1)]),
    )
}

/// Regression for #811: no Parlor on the board is a **no-op**, not a
/// rejection: `Done`, no events, nothing moved. The reachable case is
/// agenda 3 turning up while the act deck is still on act 1 or 2 — see the
/// function's own doc for why. `run_done` already asserts the `Done`.
#[test]
fn no_parlor_in_play_moves_nothing_and_does_not_reject() {
    let mut state = star_board();
    state.locations.remove(&LocationId(5)); // the Parlor
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
    let events = run_done(&mut state, move_ghouls_toward_parlor);
    assert!(events.is_empty(), "no events: {events:?}");
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3)),
        "the Ghoul stays put — there is no destination to step toward",
    );
}

#[test]
fn unengaged_ghoul_in_attic_steps_to_hallway() {
    let mut state = star_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
    let events = run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert!(events.iter().any(|e| matches!(e,
        Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(2))));
}

#[test]
fn ghoul_in_hallway_steps_to_parlor() {
    let mut state = star_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(5))
    );
}

#[test]
fn non_elite_ghoul_does_not_move_into_a_barricaded_parlor() {
    // A Barricade (01038) on the Parlor blocks the non-Elite Ghoul's
    // compelled step, so it does not move. Needs the real registry so the
    // attachment's `EnemyMovementBlocked` restriction is read.
    let mut state = star_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    barricade(&mut state, LocationId(5));
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
        "Ghoul stayed in the Hallway — the only step toward the Parlor is blocked",
    );
}

/// A detour board: Attic -> Hallway -> Parlor is the shortest route
/// (2 steps); Attic -> Study -> Cellar -> Parlor is the long way (3).
/// Synthetic topology on real Gathering codes — the printed map is the
/// star in [`star_board`], which has no detour to test against.
fn detour_board() -> GameState {
    let loc =
        |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut state = GameStateBuilder::new()
        .with_registries(crate::TEST_REGISTRIES)
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(3, "01113", "Attic"))
        .with_location(loc(4, "01114", "Cellar"))
        .with_location(loc(5, "01115", "Parlor"))
        .with_location(loc(6, "01111", "Study"))
        .build();
    state.connect(LocationId(3), LocationId(2));
    state.connect(LocationId(2), LocationId(5));
    state.connect(LocationId(3), LocationId(6));
    state.connect(LocationId(6), LocationId(4));
    state.connect(LocationId(4), LocationId(5));
    state
}

fn barricade(state: &mut GameState, at: LocationId) {
    // A seat cannot place a location attachment; mint its id from the
    // state's counter rather than hand-pick one.
    let id = state.card_instance_ids.mint();
    state
        .locations
        .get_mut(&at)
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(
            CardCode::new("01038"),
            id,
            Owner::Investigator(InvestigatorId(1)),
        ));
}

/// Regression for #797: a barricaded compelled step is a non-move, not a
/// detour. `glossary/Patrol.md`: *"If an enemy with patrol would be
/// compelled to move to a location which is blocked by a card ability, the
/// enemy does not move."* 01107 is structurally the same fixed-destination,
/// shortest-path mover.
#[test]
fn ghoul_does_not_reroute_around_a_barricaded_shortest_step() {
    let mut state = detour_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
    barricade(&mut state, LocationId(2)); // Hallway — the only shortest step

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3)),
        "the Ghoul stays put rather than taking the long way through the Study",
    );
    assert!(!events.iter().any(|e| matches!(e, Event::EnemyMoved { .. })));
}

#[test]
fn ghoul_takes_the_shortest_step_when_nothing_is_barricaded() {
    let mut state = detour_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
    );
}

#[test]
fn a_barricade_off_the_shortest_path_does_not_affect_the_move() {
    let mut state = detour_board();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic
    barricade(&mut state, LocationId(6)); // Study — on the long route only
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
    );
}

/// Regression for #633: a Ghoul that steps into a lone investigator's
/// location engages on arrival. `Enemy_Engagement.md`: *"Any time a ready unengaged
/// enemy is at the same location as an investigator, it engages that
/// investigator"*, listed example *"It moves into the same location as
/// an investigator"*.
#[test]
fn ghoul_moving_into_the_investigators_location_engages_on_arrival() {
    let mut state = star_board_with_investigator_in_hallway();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(3))); // Attic

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
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
    let mut state = star_board_with_investigator_in_hallway();
    let mut evaded = ghoul(1, LocationId(3)); // Attic
    evaded.exhausted = true;
    state.enemies.insert(EnemyId(1), evaded);

    let events = run_done(&mut state, move_ghouls_toward_parlor);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
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
    let mut state = star_board();
    let mut engaged = ghoul(1, LocationId(3));
    engaged.engaged_with = Some(InvestigatorId(1));
    state.enemies.insert(EnemyId(1), engaged);
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5))); // already at Parlor
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
    assert_eq!(
        state.enemies[&EnemyId(2)].current_location,
        Some(LocationId(5))
    );
}

#[test]
fn non_ghoul_does_not_move() {
    let mut state = star_board();
    // Swarm of Rats 01159 (Creature.): traits, but not Ghoul.
    let mut e = test_support::test_enemy(1, "Rat");
    e.code = CardCode::new("01159");
    e.current_location = Some(LocationId(3));
    state.enemies.insert(EnemyId(1), e);
    run_done(&mut state, move_ghouls_toward_parlor);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
}
