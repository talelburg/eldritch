use super::*;

#[test]
fn move_with_ready_engaged_enemy_fires_aoo_and_enemy_follows() {
    let (inv_id, a, b, enemy_id, state) = move_scenario_with_engaged_enemy();
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
    // AoO damage must fire BEFORE the move resolves per the Rules
    // Reference. assert_event! is existence-only, so check the
    // positions explicitly.
    let damage_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { .. }))
        .expect("DamageTaken event missing");
    let moved_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::InvestigatorMoved { .. }))
        .expect("InvestigatorMoved event missing");
    assert!(
        damage_idx < moved_idx,
        "AoO DamageTaken (idx {damage_idx}) must precede InvestigatorMoved (idx {moved_idx})"
    );
    // Investigator damaged.
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    // Investigator moved.
    assert_eq!(
        result.state.investigators[&inv_id].current_location,
        Some(b)
    );
    assert_event!(
        result.events,
        Event::InvestigatorMoved { from, to, .. } if *from == a && *to == b
    );
    // Engaged enemy followed.
    assert_eq!(result.state.enemies[&enemy_id].current_location, Some(b));
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
    // AoO does NOT exhaust per the Rules Reference.
    assert!(!result.state.enemies[&enemy_id].exhausted);
}

#[test]
fn move_with_exhausted_engaged_enemy_does_not_fire_aoo() {
    let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
    state.enemies.get_mut(&enemy_id).unwrap().exhausted = true;
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_no_event!(result.events, Event::HorrorTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
    // Exhausted enemy still follows the investigator.
    assert_eq!(result.state.enemies[&enemy_id].current_location, Some(b));
}

#[test]
fn draw_action_fires_aoo_from_ready_engaged_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut enemy = test_support::test_enemy(200, "Engaged Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            // Give the deck a card so the draw itself succeeds without
            // the empty-deck horror path muddying the AoO assertion.
            i.deck = vec![CardCode::new("_test_card_1")];
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Draw {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}

#[test]
fn draw_with_lethal_aoo_suppresses_the_draw() {
    // A lethal AoO (attack_damage == max_health) defeats the investigator
    // before the card is drawn; the draw is suppressed (no CardsDrawn event)
    // while the AoO damage still lands. Action is still spent.
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut enemy = test_support::test_enemy(200, "Lethal Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 8; // == test_investigator max_health
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i.deck = vec![CardCode::new("_test_card_1")];
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Draw {
            investigator: inv_id,
        },
    );

    // Lethal AoO landed.
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 8 } if *investigator == inv_id
    );
    // Action was spent before the lethal AoO (3 → 2).
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // ...but the draw was suppressed.
    assert_no_event!(result.events, Event::CardsDrawn { .. });
}

#[test]
fn draw_with_no_engaged_enemy_draws_normally() {
    // Behaviour-preserving: no AoO enemy, so the draw resolves without
    // interruption. One card drawn, Done.
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i.deck = vec![CardCode::new("_test_card_1")];
            i
        })
        .with_active_investigator(inv_id)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();

    let hand_before = state.investigators[&inv_id].hand.len();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Draw {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&inv_id].hand.len(),
        hand_before + 1,
        "one card drawn"
    );
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 1 } if *investigator == inv_id
    );
    assert_no_event!(result.events, Event::DamageTaken { .. });
}

#[test]
fn move_with_unengaged_enemy_at_origin_leaves_enemy_behind() {
    let (inv_id, a, b, _, mut state) = move_scenario_with_engaged_enemy();
    // Convert the engagement into a non-engagement: enemy is at A
    // but not engaged with anyone.
    let other_id = EnemyId(201);
    let mut other = test_support::test_enemy(201, "Bystander");
    other.current_location = Some(a);
    // engaged_with stays None.
    state.enemies.insert(other_id, other);
    // Remove the engaged enemy so the move doesn't trigger AoO,
    // keeping the focus on the unengaged enemy.
    state.enemies.remove(&EnemyId(200));

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // Investigator moved.
    assert_eq!(
        result.state.investigators[&inv_id].current_location,
        Some(b)
    );
    // Unengaged enemy stayed put.
    assert_eq!(result.state.enemies[&other_id].current_location, Some(a));
}

#[test]
fn investigate_with_ready_engaged_enemy_fires_aoo() {
    // Set up an Investigate scenario, then attach an engaged
    // enemy at the investigator's location.
    let (inv_id, loc_id, state) = investigate_scenario(2, 2);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Engaged at Study");
    enemy.current_location = Some(loc_id);
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 0;
    enemy.attack_horror = 1;
    let mut state = state;
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
    // Skill test still runs after AoO.
    assert_event!(result.events, Event::SkillTestStarted { .. });
    assert_eq!(result.state.investigators[&inv_id].horror(), 1);
}

#[test]
fn fight_does_not_fire_aoo_from_other_engaged_enemy() {
    // Investigator engaged with the Fight target AND a second
    // ready engaged enemy. Fight is on the AoO-exempt list, so
    // no AoO fires — neither from the target nor from the
    // bystander.
    let (inv_id, target_id, mut state) = fight_evade_scenario();
    let bystander_id = EnemyId(202);
    let mut bystander = test_support::test_enemy(202, "Other Ghoul");
    bystander.engaged_with = Some(inv_id);
    bystander.attack_damage = 5;
    state.enemies.insert(bystander_id, bystander);
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: target_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_no_event!(result.events, Event::HorrorTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
    assert_eq!(result.state.investigators[&inv_id].horror(), 0);
}

#[test]
fn evade_does_not_fire_aoo_from_other_engaged_enemy() {
    let (inv_id, target_id, mut state) = fight_evade_scenario();
    let bystander_id = EnemyId(203);
    let mut bystander = test_support::test_enemy(203, "Other Ghoul");
    bystander.engaged_with = Some(inv_id);
    bystander.attack_damage = 5;
    state.enemies.insert(bystander_id, bystander);
    let result = take_action_no_commits(
        state,
        &TurnAction::Evade {
            investigator: inv_id,
            enemy: target_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_no_event!(result.events, Event::HorrorTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
}

#[test]
fn move_with_no_engaged_enemy_does_not_fire_aoo() {
    // Regression: the AoO step is a no-op when no engaged
    // enemies exist; pre-existing Move tests should not have
    // started failing.
    let (inv_id, _, b, state) = move_scenario();
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::DamageTaken { .. });
}

#[test]
fn aoo_order_pick_resolves_attacks_in_chosen_order_for_multiple_attackers() {
    // 3 engaged ready enemies provoke an AoO on Move; with 2+ attackers the
    // player picks the order one at a time (#143, RR p.25 step 3.3). Pick the
    // highest-damage enemy first to prove the pick overrides EnemyId order.
    let (inv_id, _, b, state) = move_scenario();
    let mut state = state;
    // TEST_INV has 8 health; total enemy damage = 1+2+4 = 7 < 8, so investigator survives.
    // (max_health is now read from the registry, not a field — see #448 cp4.)
    for (id, dmg) in [(300, 1), (301, 2), (302, 4)] {
        let mut e = test_support::test_enemy(id, "");
        e.engaged_with = Some(inv_id);
        e.attack_damage = dmg;
        state.enemies.insert(EnemyId(id), e);
    }
    // Move provokes the AoO; 3 engaged → order pick (no attack dealt yet).
    let r1 = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }));
    let pick_302 = attack_order_pick(&r1.outcome, EnemyId(302));

    // Pick EnemyId(302) (dmg 4) first → resolves it, then re-prompts over the
    // remaining [300, 301].
    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_302),
        }),
    );
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));
    let pick_301 = attack_order_pick(&r2.outcome, EnemyId(301));

    // Pick EnemyId(301) (dmg 2); EnemyId(300) (dmg 1) is then forced. The AoO
    // loop drains and the parked Move completes.
    let r3 = apply(
        r2.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick_301),
        }),
    );
    assert!(matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }));

    let damages: Vec<u8> = r1
        .events
        .iter()
        .chain(&r2.events)
        .chain(&r3.events)
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        damages,
        vec![4, 2, 1],
        "chosen order: 302 (dmg4), 301 (dmg2), 300 (dmg1)"
    );
    assert_eq!(r3.state.investigators[&inv_id].damage(), 7);
    // The Move completed once the AoO loop drained.
    assert_event!(r3.events, Event::InvestigatorMoved { .. });
}

#[test]
fn aoo_from_zero_damage_zero_horror_enemy_emits_no_events() {
    // Edge: an engaged ready enemy with attack_damage = 0 and
    // attack_horror = 0 still "attacks" but the helper's `if > 0`
    // guards must skip both event emissions.
    let (inv_id, _, b, state) = move_scenario();
    let mut state = state;
    let mut e = test_support::test_enemy(310, "Quiet Watcher");
    e.engaged_with = Some(inv_id);
    e.attack_damage = 0;
    e.attack_horror = 0;
    state.enemies.insert(EnemyId(310), e);
    let result = test_support::take_turn_action(
        state,
        &TurnAction::Move {
            investigator: inv_id,
            destination: b,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_no_event!(result.events, Event::HorrorTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
    assert_eq!(result.state.investigators[&inv_id].horror(), 0);
}
