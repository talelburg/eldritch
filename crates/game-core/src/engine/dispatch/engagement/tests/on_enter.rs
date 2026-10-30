//! [`engage_ready_enemies_on_enter`], driven by a real Move.
//!
//! A `Prey::BearerOnly` enemy engages its bearer when they walk in.
//! `glossary/Prey.md`: *"that enemy only moves towards and engages that
//! investigator (as if it were the only investigator in play), and ignores all
//! other investigators while moving and engaging."* That a non-bearer walking
//! in is not engaged is driven through `apply` by
//! `crates/cards/tests/integration/mob_enforcer.rs`.

use super::*;
use crate::assert_event;
use crate::engine::enumerate::TurnAction;
use crate::state::GameStateBuilder;
use crate::test_support::{self, enemy, location, TEST_REGISTRIES};

const HALLWAY: LocationId = LocationId(1);
const STUDY: LocationId = LocationId(2);
const BEARER: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(1);

/// Its bearer moving into its location is engaged, as by any ready enemy.
#[test]
fn its_bearer_entering_is_engaged_by_a_bearer_only_enemy() {
    let hallway = location(HALLWAY).connections(vec![STUDY]);
    let study = location(STUDY).connections(vec![HALLWAY]);
    let enemy = enemy(ENEMY)
        .prey(Prey::BearerOnly)
        .owner(Owner::Investigator(BEARER))
        .at(STUDY);

    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(hallway)
        .with_location(study)
        .with_seat(test_support::seat(BEARER.0).at(HALLWAY))
        .with_enemy(enemy)
        .open_turn(BEARER)
        .session()
        .take(&TurnAction::Move {
            investigator: BEARER,
            destination: STUDY,
        });

    session.assert_at_turn_menu(BEARER);
    assert_eq!(session.state().enemies[&ENEMY].engaged_with, Some(BEARER));
    assert_event!(session.events(), Event::EnemyEngaged { enemy, investigator }
        if *enemy == ENEMY && *investigator == BEARER);
}
