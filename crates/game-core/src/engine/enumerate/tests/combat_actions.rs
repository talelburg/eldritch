use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

/// The investigator's location.
const HERE: LocationId = LocationId(10);
/// A second location, away from the investigator.
const ELSEWHERE: LocationId = LocationId(11);

/// An enemy engaged with investigator 1 at `loc`, ready.
fn engaged_enemy(id: u32, loc: LocationId) -> StagedEnemy {
    enemy(id).engaged_with(InvestigatorId(1)).at(loc)
}

/// Investigator 1's open turn at [`HERE`], the Study.
fn open_turn_here() -> GameStateBuilder {
    GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE))
        .open_turn(InvestigatorId(1))
}

#[test]
fn fight_and_evade_offered_for_each_engaged_enemy() {
    let state = open_turn_here().with_enemy(engaged_enemy(7, HERE)).build();

    let actions = legal_actions(&state, &TEST_REGISTRY);
    assert!(actions.contains(&TurnAction::Fight {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
    assert!(actions.contains(&TurnAction::Evade {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
}

/// Regression for #401: Fight targets any co-located enemy, engaged or not.
#[test]
fn fight_but_not_evade_for_an_unengaged_co_located_enemy() {
    // Fight targets any co-located enemy (RR p.12); Evade is
    // engagement-only (RR p.11). An unengaged enemy at the investigator's
    // location is a Fight target but not an Evade target.
    // co-located, but engaged with nobody
    let e = enemy(7).at(HERE);
    let state = open_turn_here().with_enemy(e).build();

    let actions = legal_actions(&state, &TEST_REGISTRY);
    assert!(actions.contains(&TurnAction::Fight {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
    assert!(!actions.contains(&TurnAction::Evade {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
}

#[test]
fn no_combat_for_an_enemy_at_a_different_location() {
    // An enemy elsewhere (and unengaged) is neither a Fight nor an Evade
    // target (Fight needs co-location).
    let e = enemy(7).at(ELSEWHERE);
    let state = open_turn_here()
        .with_location(location(ELSEWHERE))
        .with_enemy(e)
        .build();
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { .. } | TurnAction::Evade { .. })));
}

#[test]
fn negative_fight_value_offers_evade_only() {
    // Malformed-but-handled: the handler rejects Fight, allows Evade.
    let e = engaged_enemy(7, HERE).fight(-1);
    let state = open_turn_here().with_enemy(e).build();

    let actions = legal_actions(&state, &TEST_REGISTRY);
    assert!(!actions.contains(&TurnAction::Fight {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
    assert!(actions.contains(&TurnAction::Evade {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
}

#[test]
fn engage_offered_for_co_located_enemy_engaged_with_another() {
    // Enemy at my location, engaged with investigator 2 → I may engage it.
    let e = enemy(7).engaged_with(InvestigatorId(2));
    let state = open_turn_here()
        // Two investigators so an enemy can be engaged with the *other* one,
        // who stands here too: an engaged enemy is at its investigator's
        // location.
        .with_seat(seat(2).at(HERE))
        .with_enemy(e)
        .build();

    assert!(
        legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::Engage {
            investigator: InvestigatorId(1),
            enemy: EnemyId(7),
        })
    );
}

#[test]
fn no_engage_for_an_enemy_already_engaged_with_me_or_elsewhere() {
    // Already engaged with me → not engageable.
    let mine = engaged_enemy(7, HERE);
    // At a different location → not engageable.
    let away = enemy(8).at(ELSEWHERE);
    let state = open_turn_here()
        .with_location(location(ELSEWHERE))
        .with_enemy(mine)
        .with_enemy(away)
        .build();

    let engages: Vec<_> = legal_actions(&state, &TEST_REGISTRY)
        .into_iter()
        .filter(|a| matches!(a, TurnAction::Engage { .. }))
        .collect();
    assert!(engages.is_empty(), "no Engage offered, got {engages:?}");
}
