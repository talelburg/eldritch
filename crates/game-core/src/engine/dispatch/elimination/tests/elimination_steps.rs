use super::*;
use crate::test_support::{enemy, location, TEST_REGISTRIES};

#[test]
fn elimination_step1_removes_controlled_and_owned_cards() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            test_support::seat(1)
                .hand(["h1", "h2"])
                .deck(["d1"])
                .discard(["x1"])
                .in_play(["p1"]),
        )
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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

    let loc = location(1).clues(1);

    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc_id).clues(2).resources(4))
        .with_location(loc)
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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

    // engaged with the about-to-die investigator
    let enemy = enemy(1).at(loc).engaged_with(dead);

    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc))
        .with_seat(test_support::seat(2).at(loc))
        .with_location(location(1))
        .with_enemy(enemy)
        .with_seating([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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

    let enemy = enemy(1).at(loc).engaged_with(dead);

    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc))
        .with_location(location(1))
        .with_enemy(enemy)
        .with_seating([dead])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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

/// Three investigators seated 1, 2, 3 (investigator 1 the stored lead) at one
/// location, on `turn`'s turn, with a ready stock enemy `EnemyId(1)` engaged
/// with `engaged`. Defeating `engaged` leaves the enemy ready and unengaged
/// beside the other two: step 3's engagement tie.
fn three_beside_an_enemy_engaged_with(
    engaged: InvestigatorId,
    turn: InvestigatorId,
) -> test_support::TestSession {
    let loc = LocationId(1);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc))
        .with_seat(test_support::seat(2).at(loc))
        .with_seat(test_support::seat(3).at(loc))
        .with_location(location(1))
        .with_enemy(enemy(1).at(loc).engaged_with(engaged))
        .with_seating([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        .open_turn(turn)
        .session()
}

/// Assert `session` rests at the lead `lead` choosing whom `EnemyId(1)`
/// engages, offered exactly `tied` (each anchored to its investigator card),
/// with the enemy still unengaged.
fn assert_lead_asked_between(
    session: &test_support::TestSession,
    lead: InvestigatorId,
    tied: [InvestigatorId; 2],
) {
    use crate::engine::outcome::{InputKind, Responder};
    let prompt = session.prompt();
    assert_eq!(prompt.kind, InputKind::PickSingle, "{prompt:?}");
    assert_eq!(prompt.responder, Responder::Lead(lead), "{prompt:?}");
    let offered: Vec<usize> = tied
        .iter()
        .map(|i| session.offered_at(&session.state().investigators[i].card_anchor()))
        .collect();
    assert_eq!(
        (offered, prompt.options.len()),
        (vec![1, 1], 2),
        "the tie offers exactly the two survivors: {prompt:?}",
    );
    assert_eq!(
        session.state().enemies[&EnemyId(1)].engaged_with,
        None,
        "nobody is engaged until the lead answers",
    );
}

/// Elimination step 3 (`glossary/Elimination.md`): *"All enemies engaged with
/// that player are placed at the location the investigator was at when he or
/// she was eliminated, unengaged but otherwise maintaining their current game
/// state."* The ready enemy then engages per prey, and with two survivors beside
/// it the lead decides (`glossary/Prey.md`: *"if multiple investigators are
/// tied in meeting these instructions, the lead investigator may decide among
/// them"*).
#[test]
fn an_eliminated_investigators_enemy_tied_between_two_survivors_asks_the_lead() {
    let (lead, dead, third) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
    let session = three_beside_an_enemy_engaged_with(dead, lead).take_damage(dead, 8);

    assert!(
        session.state().investigators[&dead].status == Status::Defeated,
        "the elimination finished before the lead is asked",
    );
    assert_lead_asked_between(&session, lead, [lead, third]);

    let anchor = session.state().investigators[&third].card_anchor();
    let session = session.pick(anchor);
    assert_eq!(
        session.state().enemies[&EnemyId(1)].engaged_with,
        Some(third)
    );
    assert_event!(session.events(), Event::EnemyEngaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == third);
}

/// Step 5: *"If the lead investigator is eliminated, the remaining players (if
/// any) choose a new lead investigator."* The lead's own enemy's tie goes to
/// the next Active investigator in player order.
#[test]
fn an_eliminated_leads_enemy_tie_is_answered_by_the_new_lead() {
    let (dead, next, third) = (InvestigatorId(1), InvestigatorId(2), InvestigatorId(3));
    let session = three_beside_an_enemy_engaged_with(dead, next).take_damage(dead, 8);

    assert_lead_asked_between(&session, next, [next, third]);

    let anchor = session.state().investigators[&next].card_anchor();
    let session = session.pick(anchor);
    assert_eq!(
        session.state().enemies[&EnemyId(1)].engaged_with,
        Some(next)
    );
}

#[test]
fn last_investigator_defeated_latches_lost_resolution() {
    // Single investigator; defeat them and assert the no-remaining-players
    // scenario-ending latch is set (Rules Reference p.10 step 6).
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .build();
    let mut events = Vec::new();

    // Defeat the only investigator by horror through the standard defeat path.
    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
        inv,
        EliminationCause::Horror,
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

    let enemy = enemy(1).at(loc).engaged_with(dead);

    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc).clues(1))
        .with_seat(test_support::seat(2).at(loc))
        .with_location(location(1))
        .with_enemy(enemy)
        .with_seating([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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

    // does not re-engage even with a co-located survivor
    let enemy = enemy(1).at(loc).engaged_with(dead).exhausted();

    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(loc))
        .with_seat(test_support::seat(2).at(loc))
        .with_location(location(1))
        .with_enemy(enemy)
        .with_seating([dead, surv])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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
    // Not at any location.
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).clues(3).resources(2))
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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
fn elimination_discards_an_encounter_owned_threat_card_at_step_four() {
    // The threat area splits by owner: a card the encounter deck owns is not
    // the eliminated investigator's, so step 1 leaves it and step 4 discards it
    // to its owner's pile. No metadata is read. The owner-removed half is
    // covered by `crates/cards/tests/integration/elimination_teardown.rs`.
    let id = InvestigatorId(1);
    // A code the game-core registry does not know: the seat gives it to the
    // encounter deck.
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).threat_area(["01165"]))
        .build();
    assert_eq!(
        state.investigators[&id].threat_area[0].owner,
        Owner::EncounterDeck
    );
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx::for_test(&mut state, &mut events),
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
        "the encounter deck's card ⇒ the encounter discard"
    );
    assert!(state.investigators[&id].removed_from_game.is_empty());
}
