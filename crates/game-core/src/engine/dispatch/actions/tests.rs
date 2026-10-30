use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::{EngineOutcome, InputKind};
use crate::event::Event;
use crate::state::{
    ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId, Status,
};
use crate::test_support::{self, enemy, location, StagedEnemy, TestSession, TEST_REGISTRIES};
use crate::{assert_event, assert_event_sequence, assert_no_event};

/// Build a Move scenario: investigator at L1, L1 connected to L2,
/// 3 actions, Investigation phase, active investigator, with one
/// engaged ready enemy at the same location.
fn move_scenario_with_enemy(
    attack_damage: u8,
    inv_health: u8,
) -> (InvestigatorId, LocationId, LocationId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let l1 = LocationId(10);
    let l2 = LocationId(11);
    let enemy_id = EnemyId(100);

    // `max_health()` reads from the registry (TEST_INV = 8), so pre-load the
    // damage to leave `inv_health` remaining: total = (8 - inv_health) +
    // attack_damage >= 8 ⟺ attack_damage >= inv_health.
    let inv = test_support::seat(1)
        .at(l1)
        .harm(8_u8.saturating_sub(inv_health), 0);

    let loc1 = location(l1).connections(vec![l2]);
    let loc2 = location(l2);

    let enemy = enemy(100)
        .at(l1)
        .engaged_with(inv_id)
        .attack_damage(attack_damage)
        .attack_horror(0);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc1)
        .with_location(loc2)
        .with_enemy(enemy)
        .open_turn(inv_id)
        .build();

    (inv_id, l1, l2, enemy_id, state)
}

#[test]
fn move_with_lethal_aoo_suppresses_relocation_but_keeps_spent_action() {
    // An engaged enemy whose AoO defeats the investigator: the move is
    // suppressed, the action point + AoO damage persist.
    // Investigator has 1 health, enemy deals 1 damage → lethal AoO.
    //
    // Note: `apply_investigator_elimination` clears `current_location` to `None`
    // on defeat — the investigator is removed from their location as part of
    // defeat resolution. The key invariant is that no `InvestigatorMoved`
    // event fires and the investigator does NOT appear at the destination.
    let (inv_id, _l1, l2, _enemy_id, state) = move_scenario_with_enemy(1, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: l2,
    });

    // The lone investigator's defeat ends the game.
    let result = session.finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Action was still spent.
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // Move is suppressed — investigator did NOT reach L2.
    assert_ne!(
        result.state.investigators[&inv_id].current_location,
        Some(l2),
        "move suppressed: investigator must not appear at destination"
    );
    assert_eq!(
        result.state.investigators[&inv_id].actions_remaining, 2,
        "action still spent"
    );
    // Investigator is no longer Active (defeated by AoO).
    assert_ne!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator not Active after lethal AoO"
    );
    // No InvestigatorMoved emitted.
    assert_no_event!(result.events, Event::InvestigatorMoved { .. });
}

#[test]
fn move_with_nonlethal_aoo_relocates_after_the_attack() {
    // Engaged enemy, 1 damage, investigator survives (8 health):
    // AoO deals damage, then the move resolves.
    // No ability in the test registry → no cancel/soak windows → no suspension.
    let (inv_id, _l1, l2, enemy_id, state) = move_scenario_with_enemy(1, 8);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: l2,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();
    // AoO damage landed.
    assert_event!(
        result.events,
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 }
            if *investigator == inv_id
    );
    // Move proceeded: investigator is at L2.
    assert_eq!(
        result.state.investigators[&inv_id].current_location,
        Some(l2),
        "investigator must have relocated to L2"
    );
    assert_event!(
        result.events,
        Event::InvestigatorMoved { investigator, from: _, to }
            if *investigator == inv_id && *to == l2
    );
    // AoO damage is visible.
    assert_eq!(
        result.state.investigators[&inv_id].damage(),
        1,
        "investigator damage == 1 after nonlethal AoO"
    );
    // Engaged enemy moved with investigator to L2.
    assert_eq!(
        result.state.enemies[&enemy_id].current_location,
        Some(l2),
        "engaged enemy must follow to L2"
    );
    // AoO does not exhaust the attacker (RR p.7).
    assert!(!result.state.enemies[&enemy_id].exhausted);
}

/// Build a two-location map (L1 → L2) with a ready, unengaged enemy waiting
/// at the destination L2. The investigator starts Active at L1 with actions
/// to spend and full health, mid-Investigation. The L2 enemy isn't engaged
/// with the mover, so the move opens no attack of opportunity.
fn move_into_enemy_scenario() -> (InvestigatorId, LocationId, EnemyId, GameState) {
    move_into_enemy_scenario_with(|ready| ready)
}

/// [`move_into_enemy_scenario`], with `edit` finishing the enemy.
fn move_into_enemy_scenario_with(
    edit: impl FnOnce(StagedEnemy) -> StagedEnemy,
) -> (InvestigatorId, LocationId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let l1 = LocationId(10);
    let l2 = LocationId(11);
    let enemy_id = EnemyId(100);

    let inv = test_support::seat(1).at(l1);

    let loc1 = location(l1).connections(vec![l2]);
    let loc2 = location(l2).connections(vec![l1]);

    let enemy = edit(enemy(100).at(l2));

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc1)
        .with_location(loc2)
        .with_enemy(enemy)
        .open_turn(inv_id)
        .build();

    (inv_id, l2, enemy_id, state)
}

#[test]
fn entering_a_location_engages_a_ready_enemy_there() {
    // `glossary/Enemy_Engagement.md`: "Any time a ready unengaged enemy is at
    // the same location as an investigator, it engages that investigator, and
    // is placed in that investigator's threat area." — for its example "An
    // investigator moves into the same location as it." (Icy Ghoul 01119
    // waiting at the Cellar — no Aloof keyword.)
    let (inv_id, l2, enemy_id, state) = move_into_enemy_scenario();

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: l2,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();
    assert_eq!(
        result.state.investigators[&inv_id].current_location,
        Some(l2),
        "investigator entered L2",
    );
    assert_eq!(
        result.state.enemies[&enemy_id].engaged_with,
        Some(inv_id),
        "the ready enemy at the destination must engage the entering investigator",
    );
    assert_event!(
        result.events,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == enemy_id && *investigator == inv_id
    );
}

#[test]
fn entering_a_location_does_not_engage_an_exhausted_enemy() {
    // Only *ready* enemies engage on entry (RR). An exhausted enemy stays
    // unengaged.
    let (inv_id, l2, enemy_id, state) = move_into_enemy_scenario_with(StagedEnemy::exhausted);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: l2,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();

    assert_eq!(
        result.state.enemies[&enemy_id].engaged_with, None,
        "an exhausted (non-ready) enemy must not auto-engage on entry",
    );
    assert_no_event!(result.events, Event::EnemyEngaged { .. });
}

#[test]
fn entering_engages_all_ready_enemies_and_leaves_already_engaged_alone() {
    // RR phrases it as *each* ready enemy: every ready, unengaged enemy at
    // the destination engages the entering investigator. An enemy already
    // engaged with another investigator keeps that engagement (an enemy
    // engages only one investigator).
    let inv_id = InvestigatorId(1);
    let other = InvestigatorId(2);
    let l1 = LocationId(10);
    let l2 = LocationId(11);
    let enemy_a = EnemyId(100);
    let enemy_b = EnemyId(101);
    let enemy_c = EnemyId(102);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).at(l1))
        // The other investigator stands at the destination, so an enemy
        // there can already be engaged with them: an engaged enemy is at its
        // investigator's location.
        .with_seat(test_support::seat(2).at(l2))
        .with_location(location(l1).connections(vec![l2]))
        .with_location(location(l2).connections(vec![l1]))
        // Two ready, unengaged enemies at the destination.
        .with_enemy(enemy(100).at(l2))
        .with_enemy(enemy(101).at(l2))
        // A third at the destination, already engaged with the other
        // investigator.
        .with_enemy(enemy(102).engaged_with(other))
        .open_turn(inv_id)
        .build();

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: inv_id,
        destination: l2,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();

    // Both ready, unengaged enemies engage the entering investigator.
    assert_eq!(result.state.enemies[&enemy_a].engaged_with, Some(inv_id),);
    assert_eq!(result.state.enemies[&enemy_b].engaged_with, Some(inv_id),);
    // The already-engaged enemy is untouched.
    assert_eq!(
        result.state.enemies[&enemy_c].engaged_with,
        Some(other),
        "an enemy already engaged elsewhere is not stolen by the entering investigator",
    );
}

/// Build an Investigate scenario: investigator at a revealed location with
/// 2 clues and shroud 2, 3 actions, Investigation phase, active. Adds a
/// ready engaged enemy with the given `attack_damage` and a chaos bag
/// with a single `Numeric(0)` token (intellect 3 vs. shroud 2 → success).
fn investigate_scenario_with_enemy(
    inv_health: u8,
    attack_damage: u8,
) -> (InvestigatorId, LocationId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let enemy_id = EnemyId(200);

    // Pre-load the damage so that max_health() (8 from TEST_INV) minus the
    // damage equals inv_health (the "remaining health" the test intended).
    let inv = test_support::seat(1)
        .at(loc_id)
        .harm(8_u8.saturating_sub(inv_health), 0);

    let loc = location(loc_id).clues(2).shroud(2);

    let enemy = enemy(200)
        .at(loc_id)
        .engaged_with(inv_id)
        .attack_damage(attack_damage)
        .attack_horror(0);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc)
        .with_enemy(enemy)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(inv_id)
        .build();

    (inv_id, loc_id, enemy_id, state)
}

#[test]
fn investigate_with_nonlethal_aoo_starts_the_test_after_the_attack() {
    // Engaged enemy deals 1 damage, investigator has 8 health (survives).
    // After the AoO, the Investigate skill test starts (AwaitingInput at
    // the commit window). Assert DamagePlacedOnInvestigator precedes the test start,
    // the investigator is still Active, and took 1 damage.
    let (inv_id, _loc_id, enemy_id, state) = investigate_scenario_with_enemy(8, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Investigate {
        investigator: inv_id,
    });

    // The session rests at the skill test's commit window.
    assert!(
        matches!(session.prompt().kind, InputKind::PickMultiple(_)),
        "expected the commit window after nonlethal AoO, got {:?}",
        session.prompt()
    );
    assert!(session.state().current_skill_test().is_some());
    // AoO damage landed before the test started — prove ordering.
    assert_event_sequence!(
        session.events(),
        Event::DamagePlacedOnInvestigator { .. },
        Event::SkillTestStarted { .. }
    );
    // Investigator is still Active.
    assert_eq!(
        session.state().investigators[&inv_id].status,
        Status::Active,
        "investigator must still be Active after nonlethal AoO"
    );
    // Investigator took 1 damage.
    assert_eq!(
        session.state().investigators[&inv_id].damage(),
        1,
        "investigator must have taken 1 damage from AoO"
    );
    // AoO does not exhaust the attacker (RR p.7).
    assert!(!session.state().enemies[&enemy_id].exhausted);
}

#[test]
fn investigate_with_lethal_aoo_suppresses_the_test() {
    // AoO deals 1 damage, investigator has 1 health → lethal → no skill
    // test starts, outcome is Done, action was spent, investigator not Active.
    let (inv_id, _loc_id, _enemy_id, state) = investigate_scenario_with_enemy(1, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Investigate {
        investigator: inv_id,
    });

    // The lone investigator's defeat ends the game.
    let result = session.finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Action was spent.
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // No skill test started.
    assert_no_event!(result.events, Event::SkillTestStarted { .. });
    // Investigator is not Active (defeated by AoO).
    assert_ne!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator must not be Active after lethal AoO"
    );
}

/// Build a Resource scenario: investigator at a location, 3 actions,
/// Investigation phase, active. Adds a ready engaged enemy with the
/// given `attack_damage` and `inv_health`.
fn resource_scenario_with_enemy(
    attack_damage: u8,
    inv_health: u8,
) -> (InvestigatorId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let enemy_id = EnemyId(300);

    // Pre-load the damage so that max_health() (8 from TEST_INV) minus the
    // damage equals inv_health (the "remaining health" the test intended).
    let inv = test_support::seat(1)
        .at(loc_id)
        .harm(8_u8.saturating_sub(inv_health), 0)
        .resources(0);

    let loc = location(loc_id);

    let enemy = enemy(300)
        .at(loc_id)
        .engaged_with(inv_id)
        .attack_damage(attack_damage)
        .attack_horror(0);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc)
        .with_enemy(enemy)
        .open_turn(inv_id)
        .build();

    (inv_id, enemy_id, state)
}

#[test]
fn resource_with_lethal_aoo_suppresses_the_gain() {
    // AoO defeats the investigator: no ResourcesGained, action spent.
    // Investigator has 1 health, enemy deals 1 damage → lethal AoO.
    let (inv_id, _enemy_id, state) = resource_scenario_with_enemy(1, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Resource {
        investigator: inv_id,
    });

    // The lone investigator's defeat ends the game.
    let result = session.finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Action was still spent.
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // No resources were gained.
    assert_no_event!(result.events, Event::ResourcesGained { .. });
    // Investigator's resource count is unchanged (still 0).
    assert_eq!(
        result.state.investigators[&inv_id].resources, 0,
        "resources must not change when AoO is lethal"
    );
    // Investigator is no longer Active (defeated by AoO).
    assert_ne!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator must not be Active after lethal AoO"
    );
}

#[test]
fn resource_with_nonlethal_aoo_still_gains() {
    // Engaged enemy deals 1 damage, investigator has 8 health (survives).
    // After the AoO the Resource gain resolves: ResourcesGained IS emitted,
    // resources incremented by 1, investigator still Active, enemy not
    // exhausted (RR p.7), investigator took 1 damage.
    let (inv_id, enemy_id, state) = resource_scenario_with_enemy(1, 8);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Resource {
        investigator: inv_id,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();
    // ResourcesGained IS emitted (primary effect ran after the AoO).
    assert_event!(
        result.events,
        Event::ResourcesGained { investigator, amount: 1 }
            if *investigator == inv_id
    );
    // Resource count incremented by 1.
    assert_eq!(
        result.state.investigators[&inv_id].resources, 1,
        "resources must increase by 1 after a nonlethal AoO"
    );
    // Investigator is still Active.
    assert_eq!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator must still be Active after nonlethal AoO"
    );
    // AoO does not exhaust the attacker (RR p.7).
    assert!(
        !result.state.enemies[&enemy_id].exhausted,
        "AoO must not exhaust the attacker (RR p.7)"
    );
    // Investigator took the AoO damage.
    assert_eq!(
        result.state.investigators[&inv_id].damage(),
        1,
        "investigator damage == 1 after nonlethal AoO"
    );
}

#[test]
fn resource_with_no_engaged_enemy_gains_normally() {
    // No engaged enemy: behaviour-preserving — resources +1, Done.
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);

    let inv = test_support::seat(1).at(loc_id).resources(2);

    let loc = location(loc_id);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc)
        .open_turn(inv_id)
        .build();

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Resource {
        investigator: inv_id,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();
    // Action was spent.
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // ResourcesGained emitted.
    assert_event!(
        result.events,
        Event::ResourcesGained { investigator, amount: 1 }
            if *investigator == inv_id
    );
    // Resource count incremented.
    assert_eq!(
        result.state.investigators[&inv_id].resources, 3,
        "resources must increase by 1"
    );
}

/// Build an Engage scenario: investigator at L1, a target enemy at L1
/// (not yet engaged), and an `AoO` enemy at L1 already engaged with the
/// investigator. Returns `(inv_id, target_id, aoo_enemy_id, state)`.
fn engage_scenario_with_aoo_enemy(
    inv_health: u8,
    aoo_damage: u8,
) -> (InvestigatorId, EnemyId, EnemyId, GameState) {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);
    let target_id = EnemyId(400);
    let aoo_enemy_id = EnemyId(401);

    // Pre-load the damage so that max_health() (8 from TEST_INV) minus the
    // damage equals inv_health (the "remaining health" the test intended).
    let inv = test_support::seat(1)
        .at(loc_id)
        .harm(8_u8.saturating_sub(inv_health), 0);

    let loc = location(loc_id);

    // The target: co-located, not yet engaged — cannot AoO.
    let target = enemy(400).at(loc_id).attack_damage(0).attack_horror(0);

    // The AoO attacker: already engaged, ready — it WILL AoO.
    let aoo_enemy = enemy(401)
        .at(loc_id)
        .engaged_with(inv_id)
        .attack_damage(aoo_damage)
        .attack_horror(0);

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(inv)
        .with_location(loc)
        .with_enemy(target)
        .with_enemy(aoo_enemy)
        .open_turn(inv_id)
        .build();

    (inv_id, target_id, aoo_enemy_id, state)
}

#[test]
fn engage_with_nonlethal_aoo_engages_after_the_attack() {
    // A second engaged enemy AoOs (1 damage); the target (co-located, not yet
    // engaged) is then engaged. Assert DamagePlacedOnInvestigator (the AoO) precedes EnemyEngaged, the
    // target's engaged_with == Some(investigator), investigator survived.
    let (inv_id, target_id, aoo_enemy_id, state) = engage_scenario_with_aoo_enemy(8, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Engage {
        investigator: inv_id,
        enemy: target_id,
    });

    session.assert_at_turn_menu(inv_id);
    let result = session.finish();
    // AoO damage landed.
    assert_event!(
        result.events,
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 }
            if *investigator == inv_id
    );
    // EnemyEngaged fired (target is now engaged).
    assert_event!(
        result.events,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == target_id && *investigator == inv_id
    );
    // AoO damage (DamagePlacedOnInvestigator) precedes the engagement.
    assert_event_sequence!(
        result.events,
        Event::DamagePlacedOnInvestigator { .. },
        Event::EnemyEngaged { .. }
    );
    // Target is now engaged with the investigator.
    assert_eq!(
        result.state.enemies[&target_id].engaged_with,
        Some(inv_id),
        "target must be engaged with the investigator after engage action"
    );
    // Investigator is still Active.
    assert_eq!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator must still be Active after nonlethal AoO"
    );
    // Investigator took 1 damage.
    assert_eq!(
        result.state.investigators[&inv_id].damage(),
        1,
        "investigator damage == 1 after nonlethal AoO"
    );
    // AoO attacker is not exhausted (RR p.7).
    assert!(!result.state.enemies[&aoo_enemy_id].exhausted);
}

#[test]
fn engage_with_lethal_aoo_suppresses_the_engagement() {
    // The other engaged enemy's AoO defeats the investigator: no EnemyEngaged.
    let (inv_id, target_id, _aoo_enemy_id, state) = engage_scenario_with_aoo_enemy(1, 1);

    let session = TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Engage {
        investigator: inv_id,
        enemy: target_id,
    });

    // The lone investigator's defeat ends the game.
    let result = session.finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Action was still spent.
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == inv_id
    );
    // No engagement — target must not be engaged.
    assert_no_event!(result.events, Event::EnemyEngaged { .. });
    assert_eq!(
        result.state.enemies[&target_id].engaged_with, None,
        "target must not be engaged when AoO is lethal"
    );
    // Investigator is not Active (defeated by AoO).
    assert_ne!(
        result.state.investigators[&inv_id].status,
        Status::Active,
        "investigator must not be Active after lethal AoO"
    );
}
