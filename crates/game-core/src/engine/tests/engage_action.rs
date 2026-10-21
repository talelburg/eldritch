use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

const TARGET: EnemyId = EnemyId(300);
const ENGAGE: TurnAction = TurnAction::Engage {
    investigator: INV,
    enemy: TARGET,
};

/// An enemy at `location`, engaged with no one.
fn enemy_at(id: u32, name: &str, location: LocationId) -> Enemy {
    let mut enemy = test_support::test_enemy(id, name);
    enemy.current_location = Some(location);
    enemy
}

/// `investigator`'s open turn at the Study.
fn engage_board(investigator: Seat) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
}

/// Whether `state`'s menu offers `INV` an Engage against `enemy`.
fn offers_engage(state: &GameState, enemy: EnemyId) -> bool {
    legal_actions(state, &TEST_REGISTRY).iter().any(
        |a| matches!(a, TurnAction::Engage { investigator, enemy: e } if *investigator == INV && *e == enemy),
    )
}

#[test]
fn engage_action_engages_unengaged_enemy_at_location() {
    let session = engage_board(seat(1).at(HERE))
        .with_enemy(enemy_at(TARGET.0, "Aloof Ghoul", HERE))
        .session()
        .take(&ENGAGE);

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().enemies[&TARGET].engaged_with, Some(INV));
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
    assert_event!(
        session.events(),
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == TARGET && *investigator == INV
    );
}

/// Regression for #656: an Engage action that takes an enemy from another
/// investigator announces the disengagement, then the engagement.
#[test]
fn engage_action_takes_an_enemy_from_another_investigator_disengaging_them_first() {
    // `glossary/Engage_Action.md`: "The enemy simultaneously disengages from
    // the previous investigator and engages the investigator performing the
    // action." Both halves are announced, the disengagement first.
    let other = InvestigatorId(2);
    let session = engage_board(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE))
        .with_enemy_engaged(test_support::test_enemy(TARGET.0, "Engaged Ghoul"), other)
        .session()
        .take(&ENGAGE);

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().enemies[&TARGET].engaged_with, Some(INV));
    assert_event_sequence!(
        session.events(),
        Event::EnemyDisengaged { enemy, investigator }
            if *enemy == TARGET && *investigator == other,
        Event::EnemyEngaged { enemy, investigator }
            if *enemy == TARGET && *investigator == INV,
    );
}

#[test]
fn engage_action_provokes_aoo_from_other_engaged_enemy_not_the_target() {
    let session = engage_board(seat(1).at(HERE))
        // Not engaged yet.
        .with_enemy(enemy_at(TARGET.0, "Target Ghoul", HERE))
        .with_enemy_engaged(attacker(301, 1, 0), INV)
        .session()
        .take(&ENGAGE);

    session.assert_at_turn_menu(INV);
    // The OTHER engaged enemy made the AoO; the target (not engaged at
    // AoO time) did not. Target ends engaged; investigator took 1 damage.
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_eq!(session.state().enemies[&TARGET].engaged_with, Some(INV));
    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn engage_action_rejects_enemy_not_at_location() {
    let hallway = LocationId(11);
    let state = engage_board(seat(1).at(HERE))
        .with_location(test_support::test_location(hallway.0, "Hallway"))
        .with_enemy(enemy_at(TARGET.0, "Distant Ghoul", hallway))
        .build();
    // Enemy at a different location → Engage is not legal.
    assert!(!offers_engage(&state, TARGET));
}

#[test]
fn engage_action_rejects_already_engaged_enemy() {
    let state = engage_board(seat(1).at(HERE))
        .with_enemy_engaged(test_support::test_enemy(TARGET.0, "Engaged Ghoul"), INV)
        .build();
    // Already engaged → Engage is not legal.
    assert!(!offers_engage(&state, TARGET));
}

#[test]
fn engage_action_rejects_unknown_enemy() {
    let state = engage_board(seat(1).at(HERE)).build();
    // Unknown enemy → Engage is not legal.
    assert!(!offers_engage(&state, EnemyId(999)));
}

#[test]
fn engage_action_rejects_no_actions_remaining() {
    let state = engage_board(seat(1).at(HERE).actions(0))
        .with_enemy(enemy_at(TARGET.0, "Ghoul", HERE))
        .build();
    // No actions remaining → Engage is not legal.
    assert!(!offers_engage(&state, TARGET));
}

#[test]
fn engage_action_rejects_when_not_active_status() {
    let mut state = engage_board(seat(1).at(HERE))
        .with_enemy(enemy_at(TARGET.0, "Ghoul", HERE))
        .build();
    // No seat setter carries status.
    state.investigators.get_mut(&INV).unwrap().status = Status::Defeated;
    // Defeated status → Engage is not legal.
    assert!(!offers_engage(&state, TARGET));
}
