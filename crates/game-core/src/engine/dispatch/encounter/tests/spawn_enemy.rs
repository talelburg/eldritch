use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey, Spawn, SpawnLocation};

use super::*;
use crate::engine::outcome::OptionId;
use crate::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId, Owner, Phase};
use crate::test_support::TEST_REGISTRIES;
use crate::{assert_event, assert_event_sequence, assert_no_event, test_support};

/// The investigator's location.
const HERE: LocationId = LocationId(10);
/// A second location, connected to [`HERE`].
const THERE: LocationId = LocationId(11);
/// The location of the engage-tie tests.
const HALL: LocationId = LocationId(1);
fn synth_enemy_metadata(spawn: Option<Spawn>) -> CardMetadata {
    enemy_metadata(
        spawn,
        HealthValue::Fixed(1),
        false,
        false,
        Prey::Default,
        1,
        1,
        0,
        0,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "one argument per enemy stat a test varies, named at each call site"
)]
fn enemy_metadata(
    spawn: Option<Spawn>,
    health: HealthValue,
    hunter: bool,
    retaliate: bool,
    prey: Prey,
    fight: u8,
    evade: u8,
    damage: u8,
    horror: u8,
    victory: Option<u8>,
) -> CardMetadata {
    CardMetadata {
        code: "_synth_enemy".into(),
        name: "Synth Enemy".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
        weakness: None,
        kind: CardKind::Enemy {
            fight,
            evade,
            damage,
            horror,
            health: Some(health),
            victory,
            spawn,
            surge: false,
            peril: false,
            hunter,
            retaliate,
            prey,
            quantity: 1,
        },
    }
}

#[test]
fn spawn_enemy_at_places_enemy_at_the_given_location_not_the_drawers() {
    // The investigator is at loc 10; spawn_enemy_at is told loc 11. The
    // enemy must land at 11 (the explicit location wins), unlike
    // spawn_enemy's investigator-location fallback.
    let mut here = test_support::test_location(HERE.0, "Here");
    here.code = CardCode("_here".into());
    let mut there = test_support::test_location(THERE.0, "There");
    there.code = CardCode("_there".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(here)
        .with_location(there)
        .with_turn_order([InvestigatorId(1)])
        .build();

    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy_at(
        &mut Cx::for_test(&mut state, &mut events),
        CardCode("_synth_enemy".into()),
        &metadata,
        THERE,
        Owner::EncounterDeck,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(
        enemy.current_location,
        Some(THERE),
        "the explicit location wins over the drawer's location",
    );
    assert_event!(
        events,
        Event::EnemySpawned { location, .. } if *location == THERE
    );
}

#[test]
fn spawn_enemy_reads_combat_stats_and_keywords_from_metadata() {
    let mut loc = test_support::test_location(HERE.0, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();

    let metadata = enemy_metadata(
        None,
        HealthValue::Fixed(5),
        true,
        true,
        Prey::Default,
        4,
        4,
        2,
        2,
        None,
    );
    let mut events = Vec::new();
    spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.fight, 4);
    assert_eq!(enemy.evade, 4);
    assert_eq!(enemy.attack_damage, 2);
    assert_eq!(enemy.attack_horror, 2);
    assert_eq!(enemy.max_health, 5);
    assert!(enemy.hunter);
    assert!(enemy.retaliate);
}

#[test]
fn spawn_enemy_scales_per_investigator_health_by_investigator_count() {
    let mut loc = test_support::test_location(HERE.0, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_seat(test_support::seat(2).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();

    let metadata = enemy_metadata(
        None,
        HealthValue::PerInvestigator(5),
        false,
        false,
        Prey::Default,
        4,
        4,
        2,
        2,
        None,
    );
    let mut events = Vec::new();
    spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.max_health, 10, "5 health × 2 investigators");
}

#[test]
fn spawn_enemy_reads_victory_from_metadata() {
    let mut loc = test_support::test_location(HERE.0, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();

    let metadata = enemy_metadata(
        None,
        HealthValue::Fixed(5),
        false,
        false,
        Prey::Default,
        4,
        4,
        2,
        2,
        Some(2),
    );
    let mut events = Vec::new();
    spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.victory, Some(2));
}

#[test]
fn spawn_at_specific_location_with_one_investigator_engages_them() {
    let mut loc = test_support::test_location(HERE.0, "Synth Loc");
    loc.code = CardCode("_synth_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // Place investigator 1 at location 10.

    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_synth_loc".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
    assert_eq!(state.enemies.len(), 1);
    let (_, enemy) = state.enemies.iter().next().unwrap();
    assert_eq!(enemy.current_location, Some(HERE));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

    assert_event_sequence!(
        events,
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode("_synth_enemy".into())
                && *location == HERE
                && *engaged_with == Some(InvestigatorId(1)),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(1),
    );
}

#[test]
fn spawn_at_specific_location_with_no_investigators_leaves_unengaged() {
    let mut loc = test_support::test_location(HERE.0, "Synth Loc");
    loc.code = CardCode("_synth_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_location(loc)
        .build();
    // Investigator 1 is NOT at location 10 (current_location is None).

    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_synth_loc".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
    let (_, enemy) = state.enemies.iter().next().unwrap();
    assert_eq!(enemy.engaged_with, None);
    // No engagement happened, so no EnemyEngaged event fires.
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn spawn_at_specific_location_discards_when_location_not_in_play() {
    // Rules Reference p.24: "If an enemy has no legal location to spawn at
    // (for example, if its spawn instruction directs it to a specific
    // location that is not in play …), it does not spawn, and is discarded
    // instead." Flesh-Eater FAQ: "place that enemy card into the encounter
    // discard pile without any further effects." So the draw does NOT reject.
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_nonexistent_loc".into()),
    }));
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    assert_eq!(outcome, EngineOutcome::Done, "discard is not a rejection");
    assert!(state.enemies.is_empty(), "the enemy does not spawn");
    assert_eq!(
        state.encounter_discard,
        vec![CardCode("_synth_enemy".into())],
        "the enemy card is placed in the encounter discard pile",
    );
}

/// Regression for #635: a printed Spawn clause the engine cannot model
/// refuses loudly rather than falling through to the no-instruction rule.
#[test]
fn spawn_with_unrepresented_instruction_rejects_without_mutating() {
    // Acolyte 01169
    // prints "Spawn - Any empty location."; the drawer's own location is
    // the one location guaranteed *not* to be empty, so the fallback
    // placement would be doubly wrong (wrong location, plus an engagement
    // that should not happen).
    let mut loc = test_support::test_location(HERE.0, "Demo");
    loc.code = CardCode("_demo_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Unrepresented("Any empty location".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    let EngineOutcome::Rejected { reason } = &outcome else {
        panic!("an unmodelled spawn instruction must reject, got {outcome:?}");
    };
    assert!(
        reason.contains("Any empty location"),
        "the rejection must name the clause it could not model: {reason}",
    );
    assert!(
        state.enemies.is_empty(),
        "a rejection leaves state unchanged — no enemy is minted",
    );
    assert!(
        state.encounter_discard.is_empty(),
        "this is not the RR \"no legal location\" discard path",
    );
    assert!(events.is_empty(), "a rejection pushes no events");
}

#[test]
fn spawn_with_no_instruction_places_at_drawing_investigators_location() {
    let mut loc = test_support::test_location(HERE.0, "Demo");
    loc.code = CardCode("_demo_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
    let (_, enemy) = state.enemies.iter().next().unwrap();
    assert_eq!(enemy.current_location, Some(HERE));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));
    // Default-spawn engagement fires the paired EnemyEngaged event.
    assert_event!(
        events,
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(1)
    );
}

#[test]
fn spawn_with_no_instruction_rejects_when_drawing_investigator_has_no_location() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    // Investigator has no current_location.
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    match outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("drawing investigator has no location"),
                "unexpected reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[test]
fn spawn_engages_sole_colocated_investigator() {
    // The single-investigator engage-on-spawn path resolves inline under the
    // shared prey resolver.
    let mut loc = test_support::test_location(HALL.0, "Hall");
    loc.code = CardCode("_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_seat(test_support::seat(1).at(HALL))
        .with_turn_order([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.engaged_with, Some(InvestigatorId(1)));
}

/// Regression for #650: an enemy with no Spawn instruction engages the
/// drawer instead of prey-resolving its engagement.
#[test]
fn no_instruction_spawn_engages_the_drawer_without_resolving_prey() {
    // `glossary/Spawn.md`: "If the enemy does not have a "Spawn –"
    // instruction, the investigator drawing the enemy spawns it engaged with
    // him/her". Two investigators share the location and the drawer is not the
    // lead, so a prey resolution would tie and prompt the lead.
    let mut loc = test_support::test_location(HALL.0, "Hall");
    loc.code = CardCode("_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_seat(test_support::seat(1).at(HALL))
        .with_seat(test_support::seat(2).at(HALL))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_mythos_draw_remaining([InvestigatorId(2)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(2),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );

    assert_eq!(outcome, EngineOutcome::Done, "no prompt for the lead");
    assert!(
        !matches!(
            state.continuations.top(),
            Some(Continuation::SpawnEngage(_))
        ),
        "no spawn-engage pick is pending",
    );
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.current_location, Some(HALL));
    assert_eq!(spawned.engaged_with, Some(InvestigatorId(2)));
    assert_event_sequence!(
        events,
        Event::EnemySpawned { engaged_with, .. } if *engaged_with == Some(InvestigatorId(2)),
        Event::EnemyEngaged { investigator, .. } if *investigator == InvestigatorId(2),
    );
}

#[test]
fn spawn_tie_suspends_for_lead_pick() {
    let mut loc = test_support::test_location(HALL.0, "Hall");
    loc.code = CardCode("_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_seat(test_support::seat(1).at(HALL))
        .with_seat(test_support::seat(2).at(HALL))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    // A `Specific` spawn: only an enemy that spawns *at a location* resolves
    // prey among the investigators there.
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_loc".into()),
    }));
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    // The tie is pushed for the `drive` loop to surface, so the spawn returns.
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::SpawnEngage(_))
    ));
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.engaged_with, None);
}

#[test]
fn resume_spawn_engage_rejects_bad_pick_and_preserves_pending() {
    // Validate-first: a pick outside the stored candidate set rejects
    // and leaves the SpawnEngage frame intact for retry, with the
    // enemy still unengaged.
    let mut loc = test_support::test_location(HALL.0, "Hall");
    loc.code = CardCode("_loc".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_seat(test_support::seat(1).at(HALL))
        .with_seat(test_support::seat(2).at(HALL))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    // A `Specific` spawn: only an enemy that spawns *at a location* resolves
    // prey among the investigators there.
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_loc".into()),
    }));
    let mut events = Vec::new();
    let _ = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::SpawnEngage(_))
    ));

    // Option id 99 is out of the co-located candidate range. The subject is
    // a malformed response no step can name, so the resume handler is called
    // with it raw.
    let outcome = hunters::resume_spawn_engage(
        &mut Cx::for_test(&mut state, &mut events),
        &InputResponse::PickSingle(OptionId(99)),
    );
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "{outcome:?}"
    );
    assert!(
        matches!(
            state.continuations.top(),
            Some(Continuation::SpawnEngage(_))
        ),
        "pending must survive a rejected pick for retry",
    );
    let enemy = state.enemies.values().next().expect("enemy still placed");
    assert_eq!(enemy.engaged_with, None, "no engagement on rejected pick");
}

#[test]
fn spawn_mints_distinct_enemy_ids() {
    let mut loc = test_support::test_location(HERE.0, "L");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(HERE))
        .with_location(loc)
        .build();
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_l".into()),
    }));
    let mut events = Vec::new();

    let _ = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    let _ = spawn_enemy(
        &mut Cx::for_test(&mut state, &mut events),
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
        Owner::EncounterDeck,
    );
    assert_eq!(
        state.enemies.len(),
        2,
        "two spawns should produce two distinct enemies"
    );
}
