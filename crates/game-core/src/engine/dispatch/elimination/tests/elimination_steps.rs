use super::*;

#[test]
fn elimination_step1_removes_controlled_and_owned_cards() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode("h1".into()), CardCode("h2".into())];
    inv.deck = vec![CardCode("d1".into())];
    inv.discard = vec![CardCode("x1".into())];
    inv.cards_in_play = vec![CardInPlay::enter_play(
        CardCode("p1".into()),
        CardInstanceId(1),
        Owner::Investigator(InvestigatorId(1)),
    )];

    let mut state = GameStateBuilder::default().with_investigator(inv).build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        id,
        EliminationCause::Damage,
    );

    let after = &state.investigators[&id];
    assert!(after.hand.is_empty(), "hand drained");
    assert!(after.deck.is_empty(), "deck drained");
    assert!(after.discard.is_empty(), "discard drained");
    assert!(after.cards_in_play.is_empty(), "cards_in_play drained");
    // All five codes landed in the removed pile (order: in-play, hand, deck, discard).
    let removed: Vec<&str> = after
        .removed_from_game
        .iter()
        .map(CardCode::as_str)
        .collect();
    assert_eq!(removed.len(), 5, "all controlled/owned cards removed");
    assert!(removed.contains(&"p1"));
    assert!(removed.contains(&"h1"));
    assert!(removed.contains(&"d1"));
    assert!(removed.contains(&"x1"));
}

#[test]
fn elimination_step2_places_clues_at_location_and_zeroes_resources() {
    let id = InvestigatorId(1);
    let loc_id = LocationId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.clues = 2;
    inv.resources = 4;

    let mut loc = test_support::test_location(1, "Study");
    loc.clues = 1;

    let mut state = GameStateBuilder::default()
        .with_investigator(inv)
        .with_location(loc)
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        id,
        EliminationCause::Damage,
    );

    assert_eq!(
        state.locations[&loc_id].clues, 3,
        "2 investigator clues added to location's 1"
    );
    assert_eq!(
        state.investigators[&id].clues, 0,
        "investigator clues cleared"
    );
    assert_eq!(
        state.investigators[&id].resources, 0,
        "resources returned to pool"
    );
    assert_event!(events, Event::LocationCluesChanged { location, new_count: 3 } if *location == loc_id);
}

#[test]
fn elimination_step3_disengages_then_reengages_ready_enemy_onto_survivor() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_support::test_investigator(1);
    dying.current_location = Some(loc);

    let mut survivor = test_support::test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead); // engaged with the about-to-die investigator
        e
    };

    let mut state = GameStateBuilder::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == dead);
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(surv),
        "ready enemy re-engages the co-located survivor"
    );
    assert_event!(events, Event::EnemyEngaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == surv);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(loc));
    assert_eq!(
        state.investigators[&dead].current_location, None,
        "eliminated => between locations"
    );
}

#[test]
fn elimination_step3_solo_defeat_leaves_enemy_unengaged() {
    let dead = InvestigatorId(1);
    let loc = LocationId(1);

    let mut dying = test_support::test_investigator(1);
    dying.current_location = Some(loc);

    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e
    };

    let mut state = GameStateBuilder::default()
        .with_investigator(dying)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == dead);
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        None,
        "no surviving co-located investigator => stays unengaged"
    );
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn last_investigator_defeated_latches_lost_resolution() {
    // Single investigator; defeat them and assert the no-remaining-players
    // scenario-ending latch is set (Rules Reference p.10 step 6).
    let inv = InvestigatorId(1);
    let mut investigator = test_support::test_investigator(1);
    // After #448 cp2a: max_sanity() reads from the registry (TEST_INV = 8).
    // Pre-load 7 horror so 1 more = 8 = max_sanity → lethal horror.
    investigator.investigator_card.accumulated_horror = 7;
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    let mut events = Vec::new();

    // Apply the final point of lethal horror through the standard defeat path.
    take_horror(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        inv,
        1,
    );

    assert_event!(events, Event::AllInvestigatorsEliminated);
    // RR Elimination step 6 is the *third* ending, not a loss: the
    // scenario ended without a resolution point being reached, and the
    // campaign guide answers it under "If no resolution was reached".
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::NoResolution),
        "no-remaining-players must latch NoResolution, not a resolution point"
    );
}

#[test]
fn elimination_runs_on_horror_defeat_too() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_support::test_investigator(1);
    dying.current_location = Some(loc);
    dying.clues = 1;

    let mut survivor = test_support::test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_support::test_enemy(1, "Whippoorwill");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e
    };

    let mut state = GameStateBuilder::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Horror,
    );

    assert_eq!(state.investigators[&dead].status, Status::Defeated);
    assert_eq!(state.locations[&loc].clues, 1, "clue placed at location");
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(surv),
        "re-engaged survivor"
    );
    assert_eq!(state.investigators[&dead].current_location, None);
}

#[test]
fn elimination_step3_exhausted_engaged_enemy_disengages_but_does_not_reengage() {
    let dead = InvestigatorId(1);
    let surv = InvestigatorId(2);
    let loc = LocationId(1);

    let mut dying = test_support::test_investigator(1);
    dying.current_location = Some(loc);

    let mut survivor = test_support::test_investigator(2);
    survivor.current_location = Some(loc);

    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = Some(dead);
        e.exhausted = true; // does not re-engage even with a co-located survivor
        e
    };

    let mut state = GameStateBuilder::default()
        .with_investigator(dying)
        .with_investigator(survivor)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == dead);
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn elimination_without_location_skips_clue_placement_and_does_not_panic() {
    // Defeated "between locations" (current_location == None): step 2
    // must skip clue placement (the clues leave play with the
    // investigator) and zero resources without panicking.
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = None;
    inv.clues = 3;
    inv.resources = 2;

    let mut state = GameStateBuilder::default().with_investigator(inv).build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        id,
        EliminationCause::Damage,
    );

    assert_eq!(
        state.investigators[&id].clues, 0,
        "clues cleared (left play)"
    );
    assert_eq!(state.investigators[&id].resources, 0, "resources returned");
    assert_no_event!(events, Event::LocationCluesChanged { .. });
}

#[test]
fn elimination_without_card_metadata_treats_threat_area_as_scenario_owned() {
    // The test registry knows no 01165 ⇒ metadata_for is None ⇒ not a
    // weakness ⇒ step 4. The weakness→removed_from_game routing needs real
    // metadata and is covered by `crates/cards/tests/elimination_teardown.rs`
    // (the test registry resolves TEST_INV only).
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.threat_area = vec![CardInPlay::enter_play(
        CardCode::new("01165"),
        CardInstanceId(1),
        Owner::EncounterDeck,
    )];

    let mut state = GameStateBuilder::default().with_investigator(inv).build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        id,
        EliminationCause::Damage,
    );

    assert!(
        state.investigators[&id].threat_area.is_empty(),
        "threat area drained"
    );
    assert_eq!(
        state.encounter_discard.len(),
        1,
        "no metadata ⇒ routed to the encounter discard"
    );
    assert!(state.investigators[&id].removed_from_game.is_empty());
}
