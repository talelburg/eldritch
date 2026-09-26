//! The relocation funnel (#633): move + engage-on-arrival in one call.

use super::*;
use crate::engine::Cx;
use crate::state::GameStateBuilder;
use crate::{assert_event, assert_no_event, test_support};

/// Two adjacent locations: the investigator in the Hallway (2), a
/// ready unengaged enemy in the Attic (1).
fn two_room_board() -> GameState {
    let inv = {
        let mut i = test_support::test_investigator(1);
        i.current_location = Some(LocationId(2));
        i
    };
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(LocationId(1));
        e.engaged_with = None;
        e
    };
    let mut state = GameStateBuilder::default()
        .with_investigator(inv)
        .with_location(test_support::test_location(1, "Attic"))
        .with_location(test_support::test_location(2, "Hallway"))
        .with_enemy(enemy)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.connect(LocationId(1), LocationId(2));
    state
}

/// [`two_room_board`] with the enemy exhausted (evaded).
fn two_room_board_with_exhausted_enemy() -> GameState {
    let mut state = two_room_board();
    state
        .enemies
        .get_mut(&EnemyId(1))
        .expect("enemy seeded by two_room_board")
        .exhausted = true;
    state
}

/// `glossary/Enemy_Engagement.md`: a ready unengaged enemy immediately
/// engages if *"It moves into the same location as an investigator"*.
#[test]
fn relocate_enemy_engages_on_arrival() {
    let mut state = two_room_board();
    let mut events = Vec::new();

    relocate_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EnemyId(1),
        LocationId(2),
    );

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
    assert_event!(events, Event::EnemyMoved { enemy, to }
        if *enemy == EnemyId(1) && *to == LocationId(2));
    assert_event!(events, Event::EnemyEngaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
}

/// *"An exhausted unengaged enemy does not engage"* — it still moves.
#[test]
fn relocate_enemy_exhausted_arrives_unengaged() {
    let mut state = two_room_board_with_exhausted_enemy();
    let mut events = Vec::new();

    relocate_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EnemyId(1),
        LocationId(2),
    );

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_event!(events, Event::EnemyMoved { enemy, to }
        if *enemy == EnemyId(1) && *to == LocationId(2));
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

/// No investigator at the destination: the enemy just moves.
#[test]
fn relocate_enemy_to_empty_location_leaves_it_unengaged() {
    let mut state = two_room_board();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .expect("investigator seeded by two_room_board")
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();

    relocate_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EnemyId(1),
        LocationId(2),
    );

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

/// An already-engaged enemy keeps its engagement — the funnel must not
/// re-target it (`reengage_at_location`'s precondition is
/// `engaged_with == None`).
#[test]
fn relocate_enemy_keeps_an_existing_engagement() {
    let mut state = two_room_board();
    let other = InvestigatorId(2);
    state.investigators.insert(other, {
        let mut i = test_support::test_investigator(2);
        i.current_location = Some(LocationId(1));
        i
    });
    state.turn_order.push(other);
    state
        .enemies
        .get_mut(&EnemyId(1))
        .expect("enemy seeded by two_room_board")
        .engaged_with = Some(other);
    let mut events = Vec::new();

    relocate_enemy(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EnemyId(1),
        LocationId(2),
    );

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(other));
    assert_no_event!(events, Event::EnemyEngaged { .. });
}
