use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

const RESOURCE: TurnAction = TurnAction::Resource { investigator: INV };

/// `INV`'s open turn at the Study, with `enemy` (if any) engaged with them.
fn resource_board(enemy: Option<StagedEnemy>) -> GameStateBuilder {
    let board = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).resources(5))
        .open_turn(INV);
    match enemy {
        Some(enemy) => board.with_enemy_engaged(enemy, INV),
        None => board,
    }
}

/// Whether `state`'s menu offers `INV` a Resource action.
fn offers_resource(state: &GameState) -> bool {
    legal_actions(state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Resource { investigator } if *investigator == INV))
}

#[test]
fn resource_action_spends_action_and_gains_one_resource() {
    let session = resource_board(None).session().take(&RESOURCE);

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].resources, 6);
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
    assert_event!(
        session.events(),
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn resource_action_fires_aoo_from_ready_engaged_enemy() {
    let session = resource_board(Some(attacker(200, 1, 0)))
        .session()
        .take(&RESOURCE);

    session.assert_at_turn_menu(INV);
    // AoO fired: investigator took 1 damage, but the resource is still gained.
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_eq!(session.state().investigators[&INV].resources, 6);
    assert_event!(
        session.events(),
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn resource_action_rejects_wrong_phase() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Mythos)
        .with_seat(seat(1))
        .with_active_investigator(INV)
        .build();
    // Mythos phase → Resource is not a legal open-turn action.
    assert!(!offers_resource(&state));
}

#[test]
fn resource_action_rejects_when_not_active_investigator() {
    let other = InvestigatorId(2);
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_seating([INV, other])
        .open_turn(other)
        .build();
    // Non-active investigator → their Resource is not legal.
    assert!(!offers_resource(&state));
}

#[test]
fn resource_action_rejects_with_no_actions_remaining() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).actions(0))
        .open_turn(INV)
        .build();
    // No actions remaining → Resource is not legal.
    assert!(!offers_resource(&state));
}

#[test]
fn resource_action_rejects_when_not_active_status() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .open_turn(INV)
        .build();
    // No seat setter carries status.
    state.investigators.get_mut(&INV).unwrap().status = Status::Defeated;
    // Defeated status → Resource is not legal.
    assert!(!offers_resource(&state));
}

#[test]
fn resource_action_aoo_that_eliminates_suppresses_the_gain() {
    // A lethal AoO (attack_damage == max_health) defeats the
    // investigator before the resource is gained; the gain is
    // suppressed (no ResourcesGained event) while the AoO damage
    // still lands.
    // 8 == test_investigator max_health.
    let result = resource_board(Some(attacker(200, 8, 0)))
        .session()
        .take(&RESOURCE)
        .finish();

    // The only investigator is defeated: the game is over.
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Lethal AoO landed.
    assert_event!(
        result.events,
        Event::DamagePlacedOnInvestigator { investigator, amount: 8 } if *investigator == INV
    );
    // ...but the resource gain was suppressed.
    assert_no_event!(result.events, Event::ResourcesGained { .. });
}
