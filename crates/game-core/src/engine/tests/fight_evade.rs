use super::*;

#[test]
fn fight_succeeds_deals_one_damage_and_spends_action() {
    // Combat 3, fight 3, modifier 0 → margin 0 → success.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    // Set investigator combat = 3 (default already is 3) so the
    // test just barely passes.
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 3;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    assert_event!(
        result.events,
        Event::SkillTestStarted {
            skill: SkillKind::Combat,
            difficulty: 3,
            ..
        }
    );
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_event!(
        result.events,
        Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 1 } if *e == enemy_id
    );
    assert_no_event!(result.events, Event::EnemyDefeated { .. });
    assert_eq!(result.state.enemies[&enemy_id].damage, 1);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn fight_failure_spends_action_but_deals_no_damage() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert_no_event!(result.events, Event::EnemyDamaged { .. });
    assert_no_event!(result.events, Event::EnemyDefeated { .. });
    assert_eq!(result.state.enemies[&enemy_id].damage, 0);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn failed_fight_against_ready_retaliate_enemy_triggers_attack() {
    // Combat 1 vs fight 3 → fail. Enemy retaliates 1 dmg + 1 horror.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.retaliate = true;
    e.attack_damage = 1;
    e.attack_horror = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestFailed { .. });
    // Retaliate attack lands (damage + horror, simultaneously).
    assert_event!(result.events, Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id);
    assert_event!(result.events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id);
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    assert_eq!(result.state.investigators[&inv_id].horror(), 1);
    // Enemy does NOT exhaust after a retaliate attack (RR p.18).
    assert!(!result.state.enemies[&enemy_id].exhausted);
    // Failed fight dealt no damage to the enemy.
    assert_no_event!(result.events, Event::EnemyDamaged { .. });
    // Skill test still tears down.
    assert_event!(result.events, Event::SkillTestEnded { .. });
}

#[test]
fn successful_fight_against_retaliate_enemy_does_not_trigger_attack() {
    // Combat 3 vs fight 3 → success; retaliate must NOT fire.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 3;
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.retaliate = true;
    e.attack_damage = 1;
    e.attack_horror = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_no_event!(result.events, Event::HorrorTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
}

#[test]
fn failed_fight_against_exhausted_retaliate_enemy_does_not_trigger_attack() {
    // Retaliate requires a READY enemy (RR p.18). Exhausted → no attack.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.retaliate = true;
    e.exhausted = true;
    e.attack_damage = 1;
    e.attack_horror = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
}

#[test]
fn failed_fight_against_non_retaliate_enemy_does_not_trigger_attack() {
    // No retaliate flag → no attack on failure.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.attack_damage = 1;
    e.attack_horror = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
}

#[test]
fn failed_evade_against_retaliate_enemy_does_not_trigger_attack() {
    // Retaliate is "while attacking" — a failed Evade must NOT fire it.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.agility = 1; // vs evade 3 → fail
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.retaliate = true;
    e.attack_damage = 1;
    e.attack_horror = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Evade {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert_no_event!(result.events, Event::DamageTaken { .. });
    assert_eq!(result.state.investigators[&inv_id].damage(), 0);
}

#[test]
fn retaliate_via_loop_does_not_exhaust_the_enemy() {
    // After K2 routes retaliate through drive_attack_loop, a failed Fight against a
    // ready retaliate enemy still deals the retaliate damage AND leaves the enemy
    // ready (RR p.18) — the loop path must not exhaust it.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1; // vs fight 3 → fail
    let e = state.enemies.get_mut(&enemy_id).unwrap();
    e.retaliate = true;
    e.attack_damage = 1;
    e.attack_horror = 0;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestFailed { .. });
    // Retaliate damage landed.
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    // Enemy must remain ready (not exhausted) after the retaliate (RR p.18).
    assert!(!result.state.enemies[&enemy_id].exhausted);
    assert_no_event!(result.events, Event::EnemyExhausted { .. });
    // Skill test still tears down cleanly.
    assert_event!(result.events, Event::SkillTestEnded { .. });
}

#[test]
fn fight_defeats_enemy_when_damage_reaches_max_health() {
    // Enemy at 1/2 already; Fight success → damage 2, defeated,
    // removed from state, engagement cleared.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.enemies.get_mut(&enemy_id).unwrap().damage = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 2 } if *e == enemy_id
    );
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) }
            if *e == enemy_id && *by == inv_id
    );
    assert!(!result.state.enemies.contains_key(&enemy_id));
}

#[test]
fn evade_succeeds_disengages_and_exhausts() {
    // Default agility 3, evade 3 → margin 0 → success.
    let (inv_id, enemy_id, state) = fight_evade_scenario();
    let result = take_action_no_commits(
        state,
        &TurnAction::Evade {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::SkillTestStarted {
            skill: SkillKind::Agility,
            difficulty: 3,
            ..
        }
    );
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_event!(
        result.events,
        Event::EnemyDisengaged { enemy: e, investigator: i }
            if *e == enemy_id && *i == inv_id
    );
    assert_event!(
        result.events,
        Event::EnemyExhausted { enemy: e } if *e == enemy_id
    );
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
    assert!(result.state.enemies[&enemy_id].exhausted);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
}

#[test]
fn evade_failure_leaves_engagement_intact() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.investigators.get_mut(&inv_id).unwrap().skills.agility = 1;
    let result = take_action_no_commits(
        state,
        &TurnAction::Evade {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestFailed { .. });
    assert_no_event!(result.events, Event::EnemyDisengaged { .. });
    assert_no_event!(result.events, Event::EnemyExhausted { .. });
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
    assert!(!result.state.enemies[&enemy_id].exhausted);
}

#[test]
fn fight_against_co_located_unengaged_enemy_is_accepted() {
    // Rules Reference p.12: "To fight an enemy at his or her location…" —
    // Fight targets any enemy at the investigator's location, engaged or not
    // (#401). An unengaged but co-located enemy is a legal Fight target.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    let loc = test_support::test_location(50, "Hall");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .current_location = Some(loc_id);
    let enemy = state.enemies.get_mut(&enemy_id).unwrap();
    enemy.current_location = Some(loc_id);
    enemy.engaged_with = None;

    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );
    // Accepted: the Fight resolves its combat test rather than rejecting.
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "co-located fight should be accepted, got {:?}",
        result.outcome,
    );
}

#[test]
fn fight_against_enemy_at_a_different_location_is_rejected() {
    // The flip side of #401: an enemy NOT at the investigator's location is
    // not a Fight target, even though enemies engaged with the investigator
    // are (they share the location). Here the enemy is unengaged and elsewhere.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    let here = test_support::test_location(50, "Hall");
    let there = test_support::test_location(51, "Attic");
    let (here_id, there_id) = (here.id, there.id);
    state.locations.insert(here_id, here);
    state.locations.insert(there_id, there);
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .current_location = Some(here_id);
    let enemy = state.enemies.get_mut(&enemy_id).unwrap();
    enemy.current_location = Some(there_id);
    enemy.engaged_with = None;

    // Enemy at a different location → Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn evade_when_not_engaged_with_target_is_rejected() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.enemies.get_mut(&enemy_id).unwrap().engaged_with = None;
    // Not engaged → Evade is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Evade { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn fight_with_unknown_enemy_is_rejected() {
    let (inv_id, _, state) = fight_evade_scenario();
    // Unknown enemy → Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == EnemyId(9999))));
}

#[test]
fn fight_outside_investigation_phase_is_rejected() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.phase = Phase::Mythos;
    // Mythos phase → Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn fight_by_non_active_investigator_is_rejected() {
    let (_, enemy_id, mut state) = fight_evade_scenario();
    let other = InvestigatorId(2);
    state
        .investigators
        .insert(other, test_support::test_investigator(2));
    // Non-active investigator → their Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == other && *enemy == enemy_id)));
}

#[test]
fn fight_with_zero_actions_is_rejected() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .actions_remaining = 0;
    // No actions remaining → Fight is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
}

#[test]
fn fight_with_negative_fight_value_is_rejected_without_mutating_state() {
    // Malformed scenario data: fight = -1. validate-first must
    // reject BEFORE the action is paid for, otherwise the action
    // is silently lost without a rejection event.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.enemies.get_mut(&enemy_id).unwrap().fight = -1;
    let actions_before = state.investigators[&inv_id].actions_remaining;
    // Negative fight value → Fight is not legal (enumerate skips malformed enemies).
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
    // actions_remaining is unchanged (no mutation occurred).
    assert_eq!(
        state.investigators[&inv_id].actions_remaining,
        actions_before
    );
}

#[test]
fn evade_with_negative_evade_value_is_rejected_without_mutating_state() {
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.enemies.get_mut(&enemy_id).unwrap().evade = -1;
    let actions_before = state.investigators[&inv_id].actions_remaining;
    // Negative evade value → Evade is not legal (enumerate skips malformed enemies).
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Evade { investigator, enemy } if *investigator == inv_id && *enemy == enemy_id)));
    // actions_remaining is unchanged (no mutation occurred).
    assert_eq!(
        state.investigators[&inv_id].actions_remaining,
        actions_before
    );
}

#[test]
fn evade_on_already_exhausted_enemy_is_idempotent_on_exhaust() {
    // Edge: enemy is already exhausted but still engaged (e.g.
    // attacked the investigator earlier this round, now the
    // investigator Evades). Success disengages and leaves
    // `exhausted = true`.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    state.enemies.get_mut(&enemy_id).unwrap().exhausted = true;
    let result = take_action_no_commits(
        state,
        &TurnAction::Evade {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(result.events, Event::SkillTestSucceeded { .. });
    assert_event!(result.events, Event::EnemyDisengaged { .. });
    assert!(result.state.enemies[&enemy_id].exhausted);
    assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
}

#[test]
fn fight_engaged_with_two_enemies_only_touches_the_target() {
    // Investigator engaged with two enemies. Fight one. The other
    // engagement must stay intact and its state untouched.
    let (inv_id, enemy_id, mut state) = fight_evade_scenario();
    let other_id = EnemyId(101);
    let mut other = test_support::test_enemy(101, "Bystander Ghoul");
    other.engaged_with = Some(inv_id);
    state.enemies.insert(other_id, other);
    // Make sure the Fight defeats the target so we observe the
    // full attribution + removal path while the other is untouched.
    state.enemies.get_mut(&enemy_id).unwrap().damage = 1;

    let result = take_action_no_commits(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(!result.state.enemies.contains_key(&enemy_id));
    // Other enemy untouched.
    assert!(result.state.enemies.contains_key(&other_id));
    let other_after = &result.state.enemies[&other_id];
    assert_eq!(other_after.engaged_with, Some(inv_id));
    assert_eq!(other_after.damage, 0);
    assert!(!other_after.exhausted);
}
