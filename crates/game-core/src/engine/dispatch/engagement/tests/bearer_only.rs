//! A `Prey::BearerOnly` enemy engages its bearer only (#1125).
//! `glossary/Prey.md`: *"that enemy only moves towards and engages that
//! investigator (as if it were the only investigator in play), and ignores all
//! other investigators while moving and engaging."*

use super::*;
use crate::engine::Cx;
use crate::state::{Enemy, GameStateBuilder, Owner};
use crate::test_support::TEST_REGISTRIES;
use crate::{assert_event, assert_no_event, test_support};

const HERE: LocationId = LocationId(1);
const BEARER: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const ENEMY: EnemyId = EnemyId(1);

/// A ready, unengaged bearer-only enemy at [`HERE`], owned by [`BEARER`].
fn bearer_only_enemy() -> Enemy {
    let mut e = test_support::test_enemy(ENEMY.0, "Weakness");
    e.prey = Prey::BearerOnly;
    e.owner = Owner::Investigator(BEARER);
    e.current_location = Some(HERE);
    e
}

/// `seats` at [`HERE`] in turn order, beside [`bearer_only_enemy`].
fn board(seats: &[InvestigatorId]) -> GameState {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy(bearer_only_enemy())
        .with_turn_order(seats.iter().copied());
    for id in seats {
        builder = builder.with_seat(test_support::seat(id.0).at(HERE));
    }
    builder.build()
}

/// An investigator other than the bearer entering its location is not engaged.
#[test]
fn a_non_bearer_entering_is_not_engaged_by_a_bearer_only_enemy() {
    let mut state = board(&[OTHER]);
    let mut events = Vec::new();

    engage_ready_enemies_on_enter(&mut Cx::for_test(&mut state, &mut events), OTHER, HERE);

    assert_eq!(state.enemies[&ENEMY].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

/// Its bearer entering is engaged, as by any ready enemy.
#[test]
fn its_bearer_entering_is_engaged_by_a_bearer_only_enemy() {
    let mut state = board(&[BEARER]);
    let mut events = Vec::new();

    engage_ready_enemies_on_enter(&mut Cx::for_test(&mut state, &mut events), BEARER, HERE);

    assert_eq!(state.enemies[&ENEMY].engaged_with, Some(BEARER));
    assert_event!(events, Event::EnemyEngaged { enemy, investigator }
        if *enemy == ENEMY && *investigator == BEARER);
}

/// The engagement rule's re-engage (readying, relocation, elimination) skips
/// the turn-order-first lead for the bearer.
#[test]
fn reengage_picks_the_bearer_over_the_lead() {
    let mut state = board(&[OTHER, BEARER]);
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), ENEMY);

    assert_eq!(state.enemies[&ENEMY].engaged_with, Some(BEARER));
}

/// Beside only investigators other than its bearer, it engages no one.
#[test]
fn reengage_without_the_bearer_engages_no_one() {
    let mut state = board(&[OTHER]);
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), ENEMY);

    assert_eq!(state.enemies[&ENEMY].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
