use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey, Spawn, SpawnLocation};

use super::*;
use crate::engine::outcome::OptionId;
use crate::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId, Owner, Phase};
use crate::{assert_event, assert_event_sequence, assert_no_event, test_support};

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

#[allow(clippy::too_many_arguments)]
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
        weakness: false,
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
    let mut here = test_support::test_location(10, "Here");
    here.code = CardCode("_here".into());
    let mut there = test_support::test_location(11, "There");
    there.code = CardCode("_there".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(here)
        .with_location(there)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy_at(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        CardCode("_synth_enemy".into()),
        &metadata,
        LocationId(11),
        Owner::EncounterDeck,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(
        enemy.current_location,
        Some(LocationId(11)),
        "the explicit location wins over the drawer's location",
    );
    assert_event!(
        events,
        Event::EnemySpawned { location, .. } if *location == LocationId(11)
    );
}

#[test]
fn spawn_enemy_reads_combat_stats_and_keywords_from_metadata() {
    let mut loc = test_support::test_location(10, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

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
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
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
    let mut loc = test_support::test_location(10, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    for id in [1, 2] {
        state
            .investigators
            .get_mut(&InvestigatorId(id))
            .unwrap()
            .current_location = Some(LocationId(10));
    }

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
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.max_health, 10, "5 health × 2 investigators");
}

#[test]
fn spawn_enemy_reads_victory_from_metadata() {
    let mut loc = test_support::test_location(10, "Loc");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

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
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );

    let enemy = state.enemies.values().next().expect("enemy spawned");
    assert_eq!(enemy.victory, Some(2));
}

#[test]
fn spawn_at_specific_location_with_one_investigator_engages_them() {
    let mut loc = test_support::test_location(10, "Synth Loc");
    loc.code = CardCode("_synth_loc".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    // Place investigator 1 at location 10.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_synth_loc".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );

    assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
    assert_eq!(state.enemies.len(), 1);
    let (_, enemy) = state.enemies.iter().next().unwrap();
    assert_eq!(enemy.current_location, Some(LocationId(10)));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

    assert_event_sequence!(
        events,
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode("_synth_enemy".into())
                && *location == LocationId(10)
                && *engaged_with == Some(InvestigatorId(1)),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(1),
    );
}

#[test]
fn spawn_at_specific_location_with_no_investigators_leaves_unengaged() {
    let mut loc = test_support::test_location(10, "Synth Loc");
    loc.code = CardCode("_synth_loc".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .build();
    // Investigator 1 is NOT at location 10 (current_location is None).

    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_synth_loc".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
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
        .with_investigator(test_support::test_investigator(1))
        .build();
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_nonexistent_loc".into()),
    }));
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    assert_eq!(outcome, EngineOutcome::Done, "discard is not a rejection");
    assert!(state.enemies.is_empty(), "the enemy does not spawn");
    assert_eq!(
        state.encounter_discard,
        vec![CardCode("_synth_enemy".into())],
        "the enemy card is placed in the encounter discard pile",
    );
}

#[test]
fn spawn_with_unrepresented_instruction_rejects_without_mutating() {
    // #635: a printed Spawn clause we cannot model must refuse loudly
    // rather than fall through to the no-instruction rule. Acolyte 01169
    // prints "Spawn - Any empty location."; the drawer's own location is
    // the one location guaranteed *not* to be empty, so the fallback
    // placement would be doubly wrong (wrong location, plus an engagement
    // that should not happen).
    let mut loc = test_support::test_location(10, "Demo");
    loc.code = CardCode("_demo_loc".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Unrepresented("Any empty location".into()),
    }));
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
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
    let mut loc = test_support::test_location(10, "Demo");
    loc.code = CardCode("_demo_loc".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();

    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );

    assert!(matches!(outcome, EngineOutcome::Done), "{outcome:?}");
    let (_, enemy) = state.enemies.iter().next().unwrap();
    assert_eq!(enemy.current_location, Some(LocationId(10)));
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
        .with_investigator(test_support::test_investigator(1))
        .build();
    // Investigator has no current_location.
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
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
    // Regression: #127's single-investigator engage-on-spawn path
    // still resolves inline under the shared prey resolver.
    let mut loc = test_support::test_location(1, "Hall");
    loc.code = CardCode("_loc".into());
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    let spawned = state.enemies.values().next().expect("one enemy");
    assert_eq!(spawned.engaged_with, Some(InvestigatorId(1)));
}

#[test]
fn spawn_tie_suspends_for_lead_pick() {
    let mut loc = test_support::test_location(1, "Hall");
    loc.code = CardCode("_loc".into());
    let mut i1 = test_support::test_investigator(1);
    i1.current_location = Some(LocationId(1));
    let mut i2 = test_support::test_investigator(2);
    i2.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_investigator(i1)
        .with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let outcome = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
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
    let mut loc = test_support::test_location(1, "Hall");
    loc.code = CardCode("_loc".into());
    let mut i1 = test_support::test_investigator(1);
    i1.current_location = Some(LocationId(1));
    let mut i2 = test_support::test_investigator(2);
    i2.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_location(loc)
        .with_investigator(i1)
        .with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    let metadata = synth_enemy_metadata(None);
    let mut events = Vec::new();
    let _ = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::SpawnEngage(_))
    ));

    // Option id 99 is out of the co-located candidate range.
    let outcome = hunters::resume_spawn_engage(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
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
    let mut loc = test_support::test_location(10, "L");
    loc.code = CardCode("_l".into());
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(loc)
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    let metadata = synth_enemy_metadata(Some(Spawn {
        location: SpawnLocation::Specific("_l".into()),
    }));
    let mut events = Vec::new();

    let _ = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    let _ = spawn_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        InvestigatorId(1),
        CardCode("_synth_enemy".into()),
        &metadata,
    );
    assert_eq!(
        state.enemies.len(),
        2,
        "two spawns should produce two distinct enemies"
    );
}
