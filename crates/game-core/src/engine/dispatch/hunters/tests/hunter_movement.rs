use super::*;
use crate::engine::Cx;
use crate::state::{EnemyId, GameStateBuilder, InvestigatorId, Location, LocationId, Owner, Phase};
use crate::test_support::TEST_REGISTRIES;
use crate::{assert_event, assert_no_event, test_support};

/// Location "A".
const A: LocationId = LocationId(1);
/// Location "B".
const B: LocationId = LocationId(2);
/// Location "C".
const C: LocationId = LocationId(3);
/// Location "D".
const D: LocationId = LocationId(4);
/// A location connected to nothing.
const ISLAND: LocationId = LocationId(9);
/// The bearer of a bearer-only hunter.
const BEARER: InvestigatorId = InvestigatorId(1);
/// An investigator other than the bearer.
const OTHER: InvestigatorId = InvestigatorId(2);
/// The bearer-only hunter.
const HUNTER: EnemyId = EnemyId(1);
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_seat(test_support::seat(1).at(C))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(ghoul)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(island)
        .with_seat(test_support::seat(1).at(A))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(e)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
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
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(A))
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
    assert_no_event!(events, Event::EnemyMoved { .. });
    assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}

// A `Prey::BearerOnly` hunter moves toward and engages its bearer only (#1125).
// `glossary/Prey.md`: *"If an enemy's prey instructions contain the word
// "only," that enemy only moves towards and engages that investigator (as if
// it were the only investigator in play), and ignores all other investigators
// while moving and engaging."* That a non-bearer beside it does not pin it is
// driven through `apply` by `crates/cards/tests/silver_twilight_acolyte.rs`.

/// A line `A - B - C - D`.
fn line() -> [Location; 4] {
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    let mut c = test_support::test_location(C.0, "C");
    let mut d = test_support::test_location(D.0, "D");
    a.connections = vec![B];
    b.connections = vec![A, C];
    c.connections = vec![B, D];
    d.connections = vec![C];
    [a, b, c, d]
}

/// A ready, unengaged bearer-only hunter at `at`, owned by [`BEARER`].
fn bearer_only_hunter(at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(HUNTER.0, "Weakness");
    e.hunter = true;
    e.prey = Prey::BearerOnly;
    e.owner = Owner::Investigator(BEARER);
    e.current_location = Some(at);
    e
}

/// The Enemy phase's hunter step over the line, with the bearer at `bearer_at`
/// and the other investigator at `other_at`.
fn hunt(enemy: Enemy, bearer_at: LocationId, other_at: LocationId) -> GameState {
    let [a, b, c, d] = line();
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_location(d)
        .with_seat(test_support::seat(BEARER.0).at(bearer_at))
        .with_seat(test_support::seat(OTHER.0).at(other_at))
        .with_turn_order([BEARER, OTHER])
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(outcome, EngineOutcome::Done);
    state
}

/// The other investigator is nearer (one step, at A) than the bearer (two
/// steps, at D), but the hunter steps toward the bearer: the bearer filter
/// runs before the nearest-distance computation.
#[test]
fn bearer_only_hunter_steps_toward_its_farther_bearer() {
    let state = hunt(bearer_only_hunter(B), D, A);
    assert_eq!(state.enemies[&HUNTER].current_location, Some(C));
    assert_eq!(state.enemies[&HUNTER].engaged_with, None);
}

/// Arriving where both investigators stand, it engages the bearer, not the
/// turn-order-first lead (the bearer is second in turn order here).
#[test]
fn bearer_only_hunter_arriving_engages_its_bearer_not_the_lead() {
    let [a, b, c, d] = line();
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_location(d)
        .with_seat(test_support::seat(BEARER.0).at(C))
        .with_seat(test_support::seat(OTHER.0).at(C))
        .with_turn_order([OTHER, BEARER])
        .with_enemy(bearer_only_hunter(B))
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(outcome, EngineOutcome::Done, "no lead prompt");
    assert_eq!(state.enemies[&HUNTER].engaged_with, Some(BEARER));
}

/// With its bearer unreachable (here: not in play), the hunter ignores the
/// other investigator entirely and does not move.
#[test]
fn bearer_only_hunter_ignores_everyone_else() {
    let [a, b, c, d] = line();
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_location(d)
        .with_seat(test_support::seat(OTHER.0).at(C))
        .with_turn_order([OTHER])
        .with_enemy(bearer_only_hunter(B))
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(state.enemies[&HUNTER].current_location, Some(B));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

/// A bearer-only enemy owned by no investigator has no bearer to single out,
/// so it hunts like `Prey::Default`: toward the nearest investigator.
#[test]
fn bearer_only_without_an_investigator_owner_hunts_like_default() {
    let mut enemy = bearer_only_hunter(B);
    enemy.owner = Owner::EncounterDeck;
    let state = hunt(enemy, D, A);
    assert_eq!(state.enemies[&HUNTER].current_location, Some(A));
    assert_eq!(state.enemies[&HUNTER].engaged_with, Some(OTHER));
}
