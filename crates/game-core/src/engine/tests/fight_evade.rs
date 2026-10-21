use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

const FIGHT: TurnAction = TurnAction::Fight {
    investigator: INV,
    enemy: GHOUL,
};
const EVADE: TurnAction = TurnAction::Evade {
    investigator: INV,
    enemy: GHOUL,
};

/// The fighter at `HERE` with 1 in `skill`, so a test of it against the
/// ghoul's 3 fails.
fn weak_in(skill: SkillKind) -> Seat {
    seat(1).at(HERE).skill(skill, 1)
}

/// The ghoul, retaliating for 1 damage and 1 horror.
fn retaliating_ghoul() -> Enemy {
    let mut enemy = ghoul();
    enemy.retaliate = true;
    enemy.attack_damage = 1;
    enemy.attack_horror = 1;
    enemy
}

/// Whether `state`'s menu offers a Fight by `investigator` against `enemy`.
fn offers_fight(state: &GameState, investigator: InvestigatorId, enemy: EnemyId) -> bool {
    legal_actions(state, &TEST_REGISTRY).iter().any(|a| {
        matches!(a, TurnAction::Fight { investigator: i, enemy: e } if *i == investigator && *e == enemy)
    })
}

/// Whether `state`'s menu offers `INV` an Evade against `GHOUL`.
fn offers_evade(state: &GameState) -> bool {
    legal_actions(state, &TEST_REGISTRY).iter().any(
        |a| matches!(a, TurnAction::Evade { investigator, enemy } if *investigator == INV && *enemy == GHOUL),
    )
}

#[test]
fn fight_succeeds_deals_one_damage_and_spends_action() {
    // Combat 3, fight 3, modifier 0 → margin 0 → success.
    let session = take_committing_nothing(fight_evade_board(seat(1).at(HERE), ghoul()), &FIGHT);

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == INV
    );
    assert_event!(
        events,
        Event::SkillTestStarted {
            skill: SkillKind::Combat,
            difficulty: 3,
            ..
        }
    );
    assert_event!(events, Event::SkillTestSucceeded { .. });
    assert_event!(
        events,
        Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 1 } if *e == GHOUL
    );
    assert_no_event!(events, Event::EnemyDefeated { .. });
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
}

#[test]
fn fight_failure_spends_action_but_deals_no_damage() {
    let session = take_committing_nothing(
        fight_evade_board(weak_in(SkillKind::Combat), ghoul()),
        &FIGHT,
    );

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    assert_no_event!(session.events(), Event::EnemyDefeated { .. });
    assert_eq!(session.state().enemies[&GHOUL].damage, 0);
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
}

#[test]
fn failed_fight_against_ready_retaliate_enemy_triggers_attack() {
    // Combat 1 vs fight 3 → fail. Enemy retaliates 1 dmg + 1 horror.
    let session = take_committing_nothing(
        fight_evade_board(weak_in(SkillKind::Combat), retaliating_ghoul()),
        &FIGHT,
    );

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(events, Event::SkillTestFailed { .. });
    // Retaliate attack lands (damage + horror, simultaneously).
    assert_event!(events, Event::DamageTaken { investigator, amount: 1 } if *investigator == INV);
    assert_event!(events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV);
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    // Enemy does NOT exhaust after a retaliate attack (RR p.18).
    assert!(!session.state().enemies[&GHOUL].exhausted);
    // Failed fight dealt no damage to the enemy.
    assert_no_event!(events, Event::EnemyDamaged { .. });
    // Skill test still tears down.
    assert_event!(events, Event::SkillTestEnded { .. });
}

#[test]
fn successful_fight_against_retaliate_enemy_does_not_trigger_attack() {
    // Combat 3 vs fight 3 → success; retaliate must NOT fire.
    let session = take_committing_nothing(
        fight_evade_board(seat(1).at(HERE), retaliating_ghoul()),
        &FIGHT,
    );

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

#[test]
fn failed_fight_against_exhausted_retaliate_enemy_does_not_trigger_attack() {
    // Retaliate requires a READY enemy (RR p.18). Exhausted → no attack.
    let mut enemy = retaliating_ghoul();
    enemy.exhausted = true;
    let session =
        take_committing_nothing(fight_evade_board(weak_in(SkillKind::Combat), enemy), &FIGHT);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

#[test]
fn failed_fight_against_non_retaliate_enemy_does_not_trigger_attack() {
    // No retaliate flag → no attack on failure.
    let mut enemy = ghoul();
    enemy.attack_damage = 1;
    enemy.attack_horror = 1;
    let session =
        take_committing_nothing(fight_evade_board(weak_in(SkillKind::Combat), enemy), &FIGHT);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

#[test]
fn failed_evade_against_retaliate_enemy_does_not_trigger_attack() {
    // Retaliate is "while attacking" — a failed Evade must NOT fire it.
    // Agility 1 vs evade 3 → fail.
    let session = take_committing_nothing(
        fight_evade_board(weak_in(SkillKind::Agility), retaliating_ghoul()),
        &EVADE,
    );

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

#[test]
fn retaliate_via_loop_does_not_exhaust_the_enemy() {
    // Retaliate runs through drive_attack_loop: a failed Fight against a ready
    // retaliate enemy deals the retaliate damage AND leaves the enemy ready
    // (RR p.18) — the loop path must not exhaust it.
    let mut enemy = retaliating_ghoul();
    enemy.attack_horror = 0;
    // Combat 1 vs fight 3 → fail.
    let session =
        take_committing_nothing(fight_evade_board(weak_in(SkillKind::Combat), enemy), &FIGHT);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    // Retaliate damage landed.
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    // Enemy must remain ready (not exhausted) after the retaliate (RR p.18).
    assert!(!session.state().enemies[&GHOUL].exhausted);
    assert_no_event!(session.events(), Event::EnemyExhausted { .. });
    // Skill test still tears down cleanly.
    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

#[test]
fn fight_defeats_enemy_when_damage_reaches_max_health() {
    // Enemy at 1/2 already; Fight success → damage 2, defeated,
    // removed from state, engagement cleared.
    let mut enemy = ghoul();
    enemy.damage = 1;
    let session = take_committing_nothing(fight_evade_board(seat(1).at(HERE), enemy), &FIGHT);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 2 } if *e == GHOUL
    );
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) }
            if *e == GHOUL && *by == INV
    );
    assert!(!session.state().enemies.contains_key(&GHOUL));
}

#[test]
fn evade_succeeds_disengages_and_exhausts() {
    // Default agility 3, evade 3 → margin 0 → success.
    let session = take_committing_nothing(fight_evade_board(seat(1).at(HERE), ghoul()), &EVADE);

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::SkillTestStarted {
            skill: SkillKind::Agility,
            difficulty: 3,
            ..
        }
    );
    assert_event!(events, Event::SkillTestSucceeded { .. });
    assert_event!(
        events,
        Event::EnemyDisengaged { enemy: e, investigator: i }
            if *e == GHOUL && *i == INV
    );
    assert_event!(
        events,
        Event::EnemyExhausted { enemy: e } if *e == GHOUL
    );
    let state = session.state();
    assert_eq!(state.enemies[&GHOUL].engaged_with, None);
    assert!(state.enemies[&GHOUL].exhausted);
    assert_eq!(state.investigators[&INV].actions_remaining, 2);
}

#[test]
fn evade_failure_leaves_engagement_intact() {
    let session = take_committing_nothing(
        fight_evade_board(weak_in(SkillKind::Agility), ghoul()),
        &EVADE,
    );

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::EnemyDisengaged { .. });
    assert_no_event!(session.events(), Event::EnemyExhausted { .. });
    assert_eq!(session.state().enemies[&GHOUL].engaged_with, Some(INV));
    assert!(!session.state().enemies[&GHOUL].exhausted);
}

/// Regression for #401: an unengaged but co-located enemy is a legal Fight
/// target.
#[test]
fn fight_against_co_located_unengaged_enemy_is_accepted() {
    // Rules Reference p.12: "To fight an enemy at his or her location…" —
    // Fight targets any enemy at the investigator's location, engaged or not.
    let mut enemy = ghoul();
    enemy.current_location = Some(HERE);
    let board = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Hall"))
        .with_enemy(enemy)
        .with_chaos_bag(bag_only_zero())
        .open_turn(INV);

    // Accepted: the Fight resolves its combat test rather than rejecting.
    let session = take_committing_nothing(board, &FIGHT);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

#[test]
fn fight_against_enemy_at_a_different_location_is_rejected() {
    // The flip side: an enemy NOT at the investigator's location is
    // not a Fight target, even though enemies engaged with the investigator
    // are (they share the location). Here the enemy is unengaged and elsewhere.
    let attic = LocationId(51);
    let mut enemy = ghoul();
    enemy.current_location = Some(attic);
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Hall"))
        .with_location(test_support::test_location(attic.0, "Attic"))
        .with_enemy(enemy)
        .open_turn(INV)
        .build();

    // Enemy at a different location → Fight is not legal.
    assert!(!offers_fight(&state, INV, GHOUL));
}

#[test]
fn evade_when_not_engaged_with_target_is_rejected() {
    let mut enemy = ghoul();
    enemy.current_location = Some(HERE);
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(test_support::test_location(HERE.0, "Test Hall"))
        .with_enemy(enemy)
        .open_turn(INV)
        .build();
    // Not engaged → Evade is not legal.
    assert!(!offers_evade(&state));
}

#[test]
fn fight_with_unknown_enemy_is_rejected() {
    let state = fight_evade_board(seat(1).at(HERE), ghoul()).build();
    // Unknown enemy → Fight is not legal.
    assert!(!offers_fight(&state, INV, EnemyId(9999)));
}

#[test]
fn fight_outside_investigation_phase_is_rejected() {
    let state = fight_evade_board(seat(1).at(HERE), ghoul())
        .with_phase(Phase::Mythos)
        .build();
    // Mythos phase → Fight is not legal.
    assert!(!offers_fight(&state, INV, GHOUL));
}

#[test]
fn fight_by_non_active_investigator_is_rejected() {
    let other = InvestigatorId(2);
    let state = fight_evade_board(seat(1).at(HERE), ghoul())
        .with_seat(seat(2))
        .build();
    // Non-active investigator → their Fight is not legal.
    assert!(!offers_fight(&state, other, GHOUL));
}

#[test]
fn fight_with_zero_actions_is_rejected() {
    let state = fight_evade_board(seat(1).at(HERE).actions(0), ghoul()).build();
    // No actions remaining → Fight is not legal.
    assert!(!offers_fight(&state, INV, GHOUL));
}

#[test]
fn fight_with_negative_fight_value_is_rejected_without_mutating_state() {
    // Malformed scenario data: fight = -1. validate-first must
    // reject BEFORE the action is paid for, otherwise the action
    // is silently lost without a rejection event.
    let mut enemy = ghoul();
    enemy.fight = -1;
    let state = fight_evade_board(seat(1).at(HERE), enemy).build();
    let actions_before = state.investigators[&INV].actions_remaining;
    // Negative fight value → Fight is not legal (enumerate skips malformed enemies).
    assert!(!offers_fight(&state, INV, GHOUL));
    // actions_remaining is unchanged (no mutation occurred).
    assert_eq!(state.investigators[&INV].actions_remaining, actions_before);
}

#[test]
fn evade_with_negative_evade_value_is_rejected_without_mutating_state() {
    let mut enemy = ghoul();
    enemy.evade = -1;
    let state = fight_evade_board(seat(1).at(HERE), enemy).build();
    let actions_before = state.investigators[&INV].actions_remaining;
    // Negative evade value → Evade is not legal (enumerate skips malformed enemies).
    assert!(!offers_evade(&state));
    // actions_remaining is unchanged (no mutation occurred).
    assert_eq!(state.investigators[&INV].actions_remaining, actions_before);
}

#[test]
fn evade_on_already_exhausted_enemy_is_idempotent_on_exhaust() {
    // Edge: enemy is already exhausted but still engaged (e.g.
    // attacked the investigator earlier this round, now the
    // investigator Evades). Success disengages and leaves
    // `exhausted = true`.
    let mut enemy = ghoul();
    enemy.exhausted = true;
    let session = take_committing_nothing(fight_evade_board(seat(1).at(HERE), enemy), &EVADE);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(session.events(), Event::EnemyDisengaged { .. });
    assert!(session.state().enemies[&GHOUL].exhausted);
    assert_eq!(session.state().enemies[&GHOUL].engaged_with, None);
}

#[test]
fn fight_engaged_with_two_enemies_only_touches_the_target() {
    // Investigator engaged with two enemies. Fight one. The other
    // engagement must stay intact and its state untouched.
    let other_id = EnemyId(101);
    // Make sure the Fight defeats the target so we observe the
    // full attribution + removal path while the other is untouched.
    let mut target = ghoul();
    target.damage = 1;
    let board = fight_evade_board(seat(1).at(HERE), target)
        .with_enemy_engaged(test_support::test_enemy(101, "Bystander Ghoul"), INV);

    let session = take_committing_nothing(board, &FIGHT);

    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert!(!state.enemies.contains_key(&GHOUL));
    // Other enemy untouched.
    assert!(state.enemies.contains_key(&other_id));
    let other_after = &state.enemies[&other_id];
    assert_eq!(other_after.engaged_with, Some(INV));
    assert_eq!(other_after.damage, 0);
    assert!(!other_after.exhausted);
}
