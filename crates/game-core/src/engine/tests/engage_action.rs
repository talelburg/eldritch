use super::*;

#[test]
fn engage_action_engages_unengaged_enemy_at_location() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Aloof Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = None;
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_enemy(enemy)
        .open_turn(inv_id)
        .build();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Engage {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    assert_event!(
        result.events,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == enemy_id && *investigator == inv_id
    );
}

#[test]
fn engage_action_provokes_aoo_from_other_engaged_enemy_not_the_target() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let target_id = EnemyId(300);
    let mut target = test_support::test_enemy(300, "Target Ghoul"); // not engaged yet
    target.current_location = Some(loc);
    let mut other = test_support::test_enemy(301, "Already-Engaged Ghoul");
    other.current_location = Some(loc);
    other.engaged_with = Some(inv_id);
    other.attack_damage = 1;
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_enemy(target)
        .with_enemy(other)
        .open_turn(inv_id)
        .build();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Engage {
            investigator: inv_id,
            enemy: target_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // The OTHER engaged enemy made the AoO; the target (not engaged at
    // AoO time) did not. Target ends engaged; investigator took 1 damage.
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    assert_eq!(result.state.enemies[&target_id].engaged_with, Some(inv_id));
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}

#[test]
fn engage_action_rejects_enemy_not_at_location() {
    let inv_id = InvestigatorId(1);
    let here = LocationId(10);
    let there = LocationId(11);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Distant Ghoul");
    enemy.current_location = Some(there);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_location(test_support::test_location(11, "Hallway"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(here);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    // Enemy at a different location → Engage is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Engage { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn engage_action_rejects_already_engaged_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Engaged Ghoul");
    enemy.current_location = Some(loc);
    enemy.engaged_with = Some(inv_id);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    // Already engaged → Engage is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Engage { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn engage_action_rejects_unknown_enemy() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i
        })
        .with_active_investigator(inv_id)
        .build();
    // Unknown enemy → Engage is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Engage { investigator, enemy } if *investigator == inv_id && *enemy == EnemyId(999))));
}

#[test]
fn engage_action_rejects_no_actions_remaining() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Ghoul");
    enemy.current_location = Some(loc);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i.actions_remaining = 0;
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    // No actions remaining → Engage is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Engage { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn engage_action_rejects_when_not_active_status() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let enemy_id = EnemyId(300);
    let mut enemy = test_support::test_enemy(300, "Ghoul");
    enemy.current_location = Some(loc);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i.status = Status::Defeated;
            i
        })
        .with_active_investigator(inv_id)
        .with_enemy(enemy)
        .build();
    // Defeated status → Engage is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Engage { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}
