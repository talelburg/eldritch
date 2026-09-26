use super::*;

#[test]
fn resource_action_spends_action_and_gains_one_resource() {
    let inv_id = InvestigatorId(1);
    let loc = LocationId(10);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_location(test_support::test_location(10, "Study"))
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.current_location = Some(loc);
            i.resources = 5;
            i
        })
        .with_active_investigator(inv_id)
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(inv_id)
        .build();

    let result = test_support::take_turn_action(
        state,
        &TurnAction::Resource {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.investigators[&inv_id].resources, 6);
    assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    assert_event!(
        result.events,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == inv_id
    );
}

#[test]
fn resource_action_fires_aoo_from_ready_engaged_enemy() {
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
        &TurnAction::Resource {
            investigator: inv_id,
        },
    );

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // AoO fired: investigator took 1 damage, but the resource is still gained.
    assert_eq!(result.state.investigators[&inv_id].damage(), 1);
    assert_eq!(result.state.investigators[&inv_id].resources, 6);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
}

#[test]
fn resource_action_rejects_wrong_phase() {
    let inv_id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv_id)
        .build();
    // Mythos phase → Resource is not a legal open-turn action.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Resource { investigator } if *investigator == inv_id)));
}

#[test]
fn resource_action_rejects_when_not_active_investigator() {
    let inv_id = InvestigatorId(1);
    let other = InvestigatorId(2);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_active_investigator(other)
        .build();
    // Non-active investigator → their Resource is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Resource { investigator } if *investigator == inv_id)));
}

#[test]
fn resource_action_rejects_with_no_actions_remaining() {
    let inv_id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.actions_remaining = 0;
            i
        })
        .with_active_investigator(inv_id)
        .build();
    // No actions remaining → Resource is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Resource { investigator } if *investigator == inv_id)));
}

#[test]
fn resource_action_rejects_when_not_active_status() {
    let inv_id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator({
            let mut i = test_support::test_investigator(1);
            i.status = Status::Defeated;
            i
        })
        .with_active_investigator(inv_id)
        .build();
    // Defeated status → Resource is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Resource { investigator } if *investigator == inv_id)));
}

#[test]
fn resource_action_aoo_that_eliminates_suppresses_the_gain() {
    // A lethal AoO (attack_damage == max_health) defeats the
    // investigator before the resource is gained; the gain is
    // suppressed (no ResourcesGained event) while the AoO damage
    // still lands.
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
        &TurnAction::Resource {
            investigator: inv_id,
        },
    );

    // Lethal AoO landed.
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 8 } if *investigator == inv_id
    );
    // ...but the resource gain was suppressed.
    assert_no_event!(result.events, Event::ResourcesGained { .. });
}
