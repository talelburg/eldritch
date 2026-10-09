use super::*;

/// Build a Move scenario with one ready engaged enemy at the
/// origin. The investigator is configured to be defeated by the
/// enemy's `AoO`: `accumulated_damage` pre-loaded to 7 (leaving 1
/// health remaining), enemy `attack_damage = 1`. Returns (inv id,
/// origin, dest, enemy id, state).
fn move_scenario_with_lethal_aoo() -> (InvestigatorId, LocationId, LocationId, EnemyId, GameState) {
    let (inv_id, a, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
    // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=1).
    // max_health() = 8 from TEST_INV; 7 + 1 = 8 = defeated.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_damage = 7;
    // attack_damage = 1 is already the default from
    // move_scenario_with_engaged_enemy, but be explicit.
    state.enemies.get_mut(&enemy_id).unwrap().attack_damage = 1;
    (inv_id, a, b, enemy_id, state)
}

#[test]
fn aoo_lethal_damage_defeats_investigator_during_move_and_cancels_move() {
    let (inv_id, a, b, enemy_id, state) = move_scenario_with_lethal_aoo();
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // Damage applied + defeat event fired.
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == inv_id
    );
    // Status flipped to Defeated.
    assert_eq!(result.state.investigators[&inv_id].status, Status::Defeated);
    // Action point still spent (the action declaration stays).
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    // Move suppressed: no InvestigatorMoved event; enemy stays at
    // the origin. The investigator's location is cleared to None
    // (elimination step 3 — they have left play).
    assert_no_event!(result.events, Event::InvestigatorMoved { .. });
    assert_eq!(
        result.state.investigators[&inv_id].current_location, None,
        "eliminated investigator has no location (left play)"
    );
    assert_eq!(result.state.enemies[&enemy_id].current_location, Some(a));
    // Single-investigator scenario, so AllInvestigatorsEliminated
    // also fires.
    assert_event!(result.events, Event::AllInvestigatorsEliminated);
}

#[test]
fn aoo_lethal_horror_defeats_investigator_during_investigate_and_cancels_test() {
    // Set up Investigate with an engaged enemy whose attack is
    // pure horror. Investigator has 1 sanity remaining (accumulated_horror=7),
    // so 1 horror drives them insane.
    let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
    // Pre-load accumulated_horror so remaining sanity = 1 (lethal with attack_horror=1).
    // max_sanity() = 8 from TEST_INV; 7 + 1 = 8 = defeated.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_horror = 7;
    let enemy_id = EnemyId(400);
    let mut enemy = test_support::test_enemy(400, "Tormenting Shade");
    enemy.current_location = Some(loc_id);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 0;
    enemy.attack_horror = 1;
    state.enemies.insert(enemy_id, enemy);
    let result = take_action_no_commits(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == inv_id
    );
    assert_eq!(result.state.investigators[&inv_id].status, Status::Defeated);
    // Skill test suppressed: no SkillTestStarted event.
    assert_no_event!(result.events, Event::SkillTestStarted { .. });
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn aoo_damage_to_active_investigator_below_threshold_does_not_defeat() {
    // Sanity check: AoO that doesn't reach max_health leaves the
    // investigator Active. Same as the existing AoO test but
    // explicit on the status field and absence of defeat events.
    // After cp2a max_health()=8 from TEST_INV registry; attack_damage=1 < 8 → survives.
    let (inv_id, _, b, _, state) = move_scenario_with_engaged_enemy();
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].status, Status::Active);
    assert_no_event!(result.events, Event::InvestigatorEliminated { .. });
    // Move proceeds.
    assert_event!(result.events, Event::InvestigatorMoved { .. });
}

#[test]
fn defeated_investigator_does_not_take_further_damage() {
    // Two engaged ready enemies, both with attack_damage = 5.
    // Investigator has 1 health remaining (accumulated_damage=7). With 2
    // engaged, the Move's AoO suspends on the order pick (#143); the
    // chosen first attacker defeats the investigator, so the active-check
    // at the loop top early-breaks before the second attacks (no re-prompt).
    let (inv_id, _, b, _, mut state) = move_scenario_with_engaged_enemy();
    // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=5).
    // max_health()=8 from TEST_INV; 7 + 5 = 12 >= 8 = defeated.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_damage = 7;
    state.enemies.get_mut(&EnemyId(200)).unwrap().attack_damage = 5;
    // Add a second engaged ready enemy.
    let mut e2 = test_support::test_enemy(201, "Second Ghoul");
    e2.engaged_with = Some(inv_id);
    e2.attack_damage = 5;
    state.enemies.insert(EnemyId(201), e2);
    let r2 = TestSession::new(state)
        .take(&TurnAction::Move {
            investigator: inv_id,
            destination: b,
        })
        .pick(OptionTarget::Enemy(EnemyId(200)))
        .finish();
    // The sole investigator was defeated, so Rules Reference p.10 step 6
    // applies — "If there are no remaining players, the scenario ends" —
    // and the engine's latch for it is `Lost`. The attack loop and the open
    // turn are therefore cancelled and the ending finalizes in this same
    // step, rather than the engine re-offering a turn menu to a table with
    // no remaining players (#566).
    assert_eq!(r2.outcome, EngineOutcome::Done, "the scenario has ended");
    assert_event!(r2.events, Event::ScenarioResolved { .. });
    assert!(
        r2.state.continuations.is_empty(),
        "no stranded frames after the ending: {:?}",
        r2.state.continuations,
    );
    // Exactly one DamageTaken event (from the chosen first AoO) and one
    // InvestigatorEliminated; the second enemy never attacks (early-break).
    assert_event_count!(r2.events, 1, Event::DamageTaken { .. });
    assert_event_count!(r2.events, 1, Event::InvestigatorEliminated { .. });
    // Only one AoO fired: pre-loaded 7 + first AoO's 5 = 12; if the
    // second AoO had fired too, damage() would be 17 instead.
    assert_eq!(r2.state.investigators[&inv_id].damage(), 12);
    // The actor was defeated mid-action → the Move's primary effect is
    // suppressed by the re-validation gate (#293 keystone).
    assert_no_event!(r2.events, Event::InvestigatorMoved { .. });
}

#[test]
fn aoo_with_lethal_damage_and_sublethal_horror_applies_both_numerically() {
    // Rules Reference page 7: damage and horror from a single
    // attack are applied simultaneously. Lethal damage MUST NOT
    // short-circuit the horror application.
    let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
    // Pre-load accumulated_damage so remaining health = 5 (lethal with attack_damage=5).
    // max_health()=8 from TEST_INV; 3 + 5 = 8 = defeated.
    // Sanity headroom: max_sanity()=8, attack_horror=1 → 0+1=1 < 8 → sub-lethal.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_damage = 3;
    let enemy = state.enemies.get_mut(&enemy_id).unwrap();
    enemy.attack_damage = 5;
    enemy.attack_horror = 1;
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // Both events fire and both numeric fields land.
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 5 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id
    );
    // Pre-loaded 3 + attack 5 = 8; horror 0 + 1 = 1.
    assert_eq!(result.state.investigators[&inv_id].damage(), 8);
    assert_eq!(result.state.investigators[&inv_id].horror(), 1);
    // Exactly one InvestigatorEliminated, caused by Damage.
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == inv_id
    );
    assert_eq!(result.state.investigators[&inv_id].status, Status::Defeated);
}

#[test]
fn aoo_with_sublethal_damage_and_lethal_horror_applies_both_numerically() {
    // Symmetric to the lethal-damage case: lethal horror MUST NOT
    // short-circuit the damage application.
    let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
    // Pre-load accumulated_horror so remaining sanity = 1 (lethal with attack_horror=5 would be
    // 7+5=12 >= 8; but we want sub-lethal damage (1 < 8) and lethal horror).
    // remaining sanity = 8 - 3 = 5, and 3 + 5 = 8 = defeated.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_horror = 3;
    let enemy = state.enemies.get_mut(&enemy_id).unwrap();
    enemy.attack_damage = 1;
    enemy.attack_horror = 5;
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 5 } if *investigator == inv_id
    );
    // damage: 0 + 1 = 1; horror: pre-loaded 3 + attack 5 = 8.
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    assert_eq!(result.state.investigators[&inv_id].horror(), 8);
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == inv_id
    );
    assert_eq!(result.state.investigators[&inv_id].status, Status::Defeated);
}

#[test]
fn aoo_with_both_lethal_defeats_once_with_damage_cause() {
    // Both stats cross their threshold from the same attack. Per
    // the enemy_attack doc comment, the tie-break is
    // EliminationCause::Damage (Rules Reference is silent on the
    // simultaneous-lethal case; damage-first is the convention).
    let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
    // Pre-load both counters so remaining health and sanity = 1 each.
    // max_health()=max_sanity()=8 from TEST_INV; 7+1=8 = defeated for both.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_damage = 7;
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .investigator_card
        .accumulated_horror = 7;
    let enemy = state.enemies.get_mut(&enemy_id).unwrap();
    enemy.attack_damage = 1;
    enemy.attack_horror = 1;
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // Pre-loaded 7 + attack 1 = 8 each.
    assert_eq!(result.state.investigators[&inv_id].damage(), 8);
    assert_eq!(result.state.investigators[&inv_id].horror(), 8);
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == inv_id
    );
    assert_eq!(result.state.investigators[&inv_id].status, Status::Defeated);
}

#[test]
fn all_investigators_eliminated_fires_only_when_last_active_falls() {
    // Two investigators, one defeated, then the second defeated.
    // AllInvestigatorsEliminated should fire only on the second.
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    let mut i1 = test_support::test_investigator(1);
    // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=1).
    // max_health()=8 from TEST_INV; 7+1=8=defeated.
    i1.investigator_card.accumulated_damage = 7;
    i1.actions_remaining = 3;
    let i2 = test_support::test_investigator(2);
    // i2 stays at default 8/8.
    let mut e = test_support::test_enemy(500, "Lethal Ghoul");
    e.engaged_with = Some(inv1);
    e.attack_damage = 1;
    let a = LocationId(10);
    let b = LocationId(11);
    let mut loc_a = test_support::test_location(10, "A");
    loc_a.connections = vec![b];
    let state = GameStateBuilder::new()
        .with_investigator(i1)
        .with_investigator(i2)
        .with_location(loc_a)
        .with_location(test_support::test_location(11, "B"))
        .with_chaos_bag(bag_only_zero())
        .with_turn_order([inv1, inv2])
        .with_enemy(e)
        .open_turn(inv1)
        .build();
    // First, place inv1 at A so the move scenario validates.
    let mut state = state;
    state.investigators.get_mut(&inv1).unwrap().current_location = Some(a);

    // inv1 moves → AoO defeats them. inv2 is still Active.
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv1,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::InvestigatorEliminated { investigator, .. } if *investigator == inv1
    );
    assert_no_event!(result.events, Event::AllInvestigatorsEliminated);
    assert_eq!(result.state.investigators[&inv1].status, Status::Defeated);
    assert_eq!(result.state.investigators[&inv2].status, Status::Active);
}

#[test]
fn defeated_investigator_cannot_move() {
    let (inv_id, _, b, _, mut state) = move_scenario_with_engaged_enemy();
    state.investigators.get_mut(&inv_id).unwrap().status = Status::Defeated;
    // Defeated status → Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == inv_id && *destination == b)));
}

#[test]
fn defeated_investigator_cannot_investigate() {
    let (inv_id, _, mut state) = investigate_scenario(2, 2);
    state.investigators.get_mut(&inv_id).unwrap().status = Status::Defeated;
    // Defeated status → Investigate is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == inv_id)));
}

#[test]
fn defeated_investigator_cannot_fight() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().status = Status::Defeated;
    // Defeated status → Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn defeated_investigator_cannot_evade() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().status = Status::Defeated;
    // Defeated status → Evade is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Evade { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn defeated_investigator_cannot_perform_skill_test() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.status = Status::Defeated;
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 0);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}
