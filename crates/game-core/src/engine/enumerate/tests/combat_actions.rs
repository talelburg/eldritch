use super::*;

/// An enemy engaged with investigator 1 at `loc`, ready.
fn engaged_enemy(id: u32, loc: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(id, "Ghoul");
    e.engaged_with = Some(InvestigatorId(1));
    e.current_location = Some(loc);
    e
}

#[test]
fn fight_and_evade_offered_for_each_engaged_enemy() {
    let mut state = open_turn_state();
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    let e = engaged_enemy(7, loc_id);
    state.enemies.insert(e.id, e);

    let actions = legal_actions(&state);
    assert!(actions.contains(&TurnAction::Fight {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
    assert!(actions.contains(&TurnAction::Evade {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
}

#[test]
fn fight_but_not_evade_for_an_unengaged_co_located_enemy() {
    // #401: Fight targets any co-located enemy (RR p.12); Evade is
    // engagement-only (RR p.11). An unengaged enemy at the investigator's
    // location is a Fight target but not an Evade target.
    let mut state = open_turn_state();
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    let mut e = test_support::test_enemy(7, "Ghoul");
    e.current_location = Some(loc_id); // co-located, but engaged with nobody
    state.enemies.insert(e.id, e);

    let actions = legal_actions(&state);
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
    // target (#401: Fight needs co-location).
    let mut state = open_turn_state();
    let here = test_support::test_location(10, "Study");
    let there = test_support::test_location(11, "Attic");
    let (here_id, there_id) = (here.id, there.id);
    state.locations.insert(here_id, here);
    state.locations.insert(there_id, there);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(here_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    let mut e = test_support::test_enemy(7, "Ghoul");
    e.current_location = Some(there_id);
    state.enemies.insert(e.id, e);
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { .. } | TurnAction::Evade { .. })));
}

#[test]
fn negative_fight_value_offers_evade_only() {
    let mut state = open_turn_state();
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    let mut e = engaged_enemy(7, loc_id);
    e.fight = -1; // malformed-but-handled: handler rejects Fight, allows Evade
    state.enemies.insert(e.id, e);

    let actions = legal_actions(&state);
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
    let mut state = open_turn_state();
    // Two investigators so an enemy can be engaged with the *other* one.
    state
        .investigators
        .insert(InvestigatorId(2), test_support::test_investigator(2));
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    // Enemy at my location, engaged with investigator 2 → I may engage it.
    let mut e = test_support::test_enemy(7, "Ghoul");
    e.current_location = Some(loc_id);
    e.engaged_with = Some(InvestigatorId(2));
    state.enemies.insert(e.id, e);

    assert!(legal_actions(&state).contains(&TurnAction::Engage {
        investigator: InvestigatorId(1),
        enemy: EnemyId(7),
    }));
}

#[test]
fn no_engage_for_an_enemy_already_engaged_with_me_or_elsewhere() {
    let mut state = open_turn_state();
    let loc = test_support::test_location(10, "Study");
    let other = test_support::test_location(11, "Hall");
    let (loc_id, other_id) = (loc.id, other.id);
    state.locations.insert(loc_id, loc);
    state.locations.insert(other_id, other);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    // Already engaged with me → not engageable.
    let mut mine = engaged_enemy(7, loc_id);
    mine.current_location = Some(loc_id);
    state.enemies.insert(mine.id, mine);
    // At a different location → not engageable.
    let mut away = test_support::test_enemy(8, "Rat");
    away.current_location = Some(other_id);
    state.enemies.insert(away.id, away);

    let engages: Vec<_> = legal_actions(&state)
        .into_iter()
        .filter(|a| matches!(a, TurnAction::Engage { .. }))
        .collect();
    assert!(engages.is_empty(), "no Engage offered, got {engages:?}");
}
