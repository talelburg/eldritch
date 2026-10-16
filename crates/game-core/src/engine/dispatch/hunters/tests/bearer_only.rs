//! A `Prey::BearerOnly` hunter moves toward and engages its bearer only
//! (#1125). `glossary/Prey.md`: *"If an enemy's prey instructions contain the
//! word "only," that enemy only moves towards and engages that investigator (as
//! if it were the only investigator in play), and ignores all other
//! investigators while moving and engaging."*

use card_dsl::card_data::Prey;

use super::*;
use crate::engine::Cx;
use crate::state::{
    Enemy, EnemyId, GameStateBuilder, InvestigatorId, Location, LocationId, Owner, Phase,
};
use crate::test_support::TEST_REGISTRIES;
use crate::{assert_no_event, test_support};

const A: LocationId = LocationId(1);
const B: LocationId = LocationId(2);
const C: LocationId = LocationId(3);
const D: LocationId = LocationId(4);
const BEARER: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HUNTER: EnemyId = EnemyId(1);

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

/// Standing with only the other investigator, the hunter is not pinned there:
/// it moves on toward its bearer and engages them on arrival.
#[test]
fn a_non_bearer_does_not_pin_a_bearer_only_hunter() {
    let state = hunt(bearer_only_hunter(B), C, B);
    assert_eq!(state.enemies[&HUNTER].current_location, Some(C));
    assert_eq!(state.enemies[&HUNTER].engaged_with, Some(BEARER));
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
