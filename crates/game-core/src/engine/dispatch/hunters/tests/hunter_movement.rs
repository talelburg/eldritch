use super::*;
use crate::engine::Cx;
use crate::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId, Phase};
use crate::{assert_event, assert_no_event, test_support};

/// Location "A".
const A: LocationId = LocationId(1);
/// Location "B".
const B: LocationId = LocationId(2);
/// Location "C".
const C: LocationId = LocationId(3);
/// A location connected to nothing.
const ISLAND: LocationId = LocationId(9);
#[test]
fn hunter_moves_one_step_toward_investigator_two_hops_away_no_engage() {
    // Map: A(1)-B(2)-C(3). Investigator at C; hunter at A. Hunter moves
    // A->B (one step). No investigator at B, so no engage yet.
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    let mut c = test_support::test_location(C.0, "C");
    a.connections = vec![B];
    b.connections = vec![A, C];
    c.connections = vec![B];
    let mut ghoul = test_support::test_enemy(1, "Swarm");
    ghoul.hunter = true;
    ghoul.current_location = Some(A);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_seat(test_support::seat(1).at(C))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(ghoul)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(B));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_event!(events, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == B);
}

#[test]
fn hunter_engages_when_it_moves_into_investigators_location() {
    // Map A(1)-B(2). Investigator at B; hunter at A. Hunter moves A->B
    // and engages on arrival.
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    a.connections = vec![B];
    b.connections = vec![A];
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(A);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(B));
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
    assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}

#[test]
fn hunter_with_no_path_does_not_move() {
    let mut a = test_support::test_location(A.0, "A");
    let island = test_support::test_location(ISLAND.0, "Island");
    a.connections = vec![];
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(ISLAND);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(island)
        .with_seat(test_support::seat(1).at(A))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(ISLAND));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn exhausted_hunter_is_skipped() {
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    a.connections = vec![B];
    b.connections = vec![A];
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.exhausted = true;
    h.current_location = Some(A);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn non_hunter_enemy_does_not_move() {
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    a.connections = vec![B];
    b.connections = vec![A];
    let mut e = test_support::test_enemy(1, "Slug");
    e.hunter = false;
    e.current_location = Some(A);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(e)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn hunter_already_co_located_does_not_move_but_engages() {
    // Hunter and investigator both at A(1). p.12: an enemy already at a
    // location with an investigator does not move; it still engages.
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    a.connections = vec![B];
    b.connections = vec![A];
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(A);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(A))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
    assert_no_event!(events, Event::EnemyMoved { .. });
    assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}
