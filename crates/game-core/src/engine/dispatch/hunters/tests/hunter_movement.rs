use super::*;
use crate::engine::Cx;
use crate::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId, Owner, Phase};
use crate::test_support::{enemy, location};
use crate::test_support::{StagedEnemy, StagedLocation, TEST_REGISTRIES};
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

/// Hunter movement finished without a tie for the lead to break. A tie pushes
/// its `HunterMove` prompt and returns `Done` too, so the outcome alone does
/// not say so; the frame on top does.
#[track_caller]
fn assert_no_tie(state: &GameState, outcome: &EngineOutcome) {
    assert_eq!(*outcome, EngineOutcome::Done);
    assert!(
        state.continuations.top_of::<HunterChoice>().is_none(),
        "no hunter tie is pending: {:?}",
        state.continuations.top(),
    );
}
#[test]
fn hunter_moves_one_step_toward_investigator_two_hops_away_no_engage() {
    // Map: A(1)-B(2)-C(3). Investigator at C; hunter at A. Hunter moves
    // A->B (one step). No investigator at B, so no engage yet.
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A, C]);
    let c = location(C).connections(vec![B]);
    let ghoul = enemy(1).hunter().at(A);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_location(c)
        .with_seat(test_support::seat(1).at(C))
        .with_seating([InvestigatorId(1)])
        .with_enemy(ghoul)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_no_tie(&state, &outcome);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(B));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_event!(events, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == B);
}

#[test]
fn hunter_engages_when_it_moves_into_investigators_location() {
    // Map A(1)-B(2). Investigator at B; hunter at A. Hunter moves A->B
    // and engages on arrival.
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A]);
    let h = enemy(1).hunter().at(A);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_seating([InvestigatorId(1)])
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
    let a = location(A).connections(vec![]);
    let island = location(ISLAND);
    let h = enemy(1).hunter().at(ISLAND);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(island)
        .with_seat(test_support::seat(1).at(A))
        .with_seating([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(ISLAND));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn exhausted_hunter_is_skipped() {
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A]);
    let h = enemy(1).hunter().exhausted().at(A);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_seating([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_no_event!(events, Event::EnemyMoved { .. });
}

#[test]
fn non_hunter_enemy_does_not_move() {
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A]);
    let e = enemy(1).at(A);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_seating([InvestigatorId(1)])
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
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A]);
    let h = enemy(1).hunter().at(A);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(A))
        .with_seating([InvestigatorId(1)])
        .with_enemy(h)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_no_tie(&state, &outcome);
    assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(A));
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
    assert_no_event!(events, Event::EnemyMoved { .. });
    assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}

// A `Prey::BearerOnly` hunter moves toward and engages its bearer only.
// `glossary/Prey.md`: *"If an enemy's prey instructions contain the word
// "only," that enemy only moves towards and engages that investigator (as if
// it were the only investigator in play), and ignores all other investigators
// while moving and engaging."* That a non-bearer beside it does not pin it is
// driven through `apply` by `crates/cards/tests/integration/silver_twilight_acolyte.rs`.

/// A line `A - B - C - D`.
fn line() -> [StagedLocation; 4] {
    let a = location(A).connections(vec![B]);
    let b = location(B).connections(vec![A, C]);
    let c = location(C).connections(vec![B, D]);
    let d = location(D).connections(vec![C]);
    [a, b, c, d]
}

/// A ready, unengaged bearer-only hunter at `at`, owned by [`BEARER`].
fn bearer_only_hunter(at: LocationId) -> StagedEnemy {
    enemy(HUNTER)
        .hunter()
        .prey(Prey::BearerOnly)
        .owner(Owner::Investigator(BEARER))
        .at(at)
}

/// The Enemy phase's hunter step over the line, with the bearer at `bearer_at`
/// and the other investigator at `other_at`.
fn hunt(enemy: StagedEnemy, bearer_at: LocationId, other_at: LocationId) -> GameState {
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
        .with_seating([BEARER, OTHER])
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_no_tie(&state, &outcome);
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
/// player-order-first lead (the bearer is second in player order here).
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
        .with_seating([OTHER, BEARER])
        .with_enemy(bearer_only_hunter(B))
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx::for_test(&mut state, &mut events));
    assert_no_tie(&state, &outcome);
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
        .with_seating([OTHER])
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
    let state = hunt(bearer_only_hunter(B).owner(Owner::EncounterDeck), D, A);
    assert_eq!(state.enemies[&HUNTER].current_location, Some(A));
    assert_eq!(state.enemies[&HUNTER].engaged_with, Some(OTHER));
}
