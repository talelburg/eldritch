//! A hunter's move is resolved by the player when the shortest path ties, and
//! that choice replays identically.
//!
//! The substrate is map **topology**, not a card: a symmetric diamond producing
//! a genuine two-way tie in the hunter's first step. A hand-built fixture is the
//! right one here — ADR 0016 permits one that models an engine primitive, which
//! a bare connection graph is. The Gathering's hub-and-spoke layout has no
//! analogue, which is why the map stays hand-built rather than flipping to real
//! scenario content.
//!
//! The test is about the tie and the `PickSingle` that settles it, not about
//! hunter movement at large. The spawn-engagement tie is tested on real cards
//! under `crates/cards/tests/`.

use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{enemy, location, seat, TEST_REGISTRIES};

/// D, where the investigator the hunter moves toward stands.
const HERE: LocationId = LocationId(4);

#[test]
fn hunter_move_tie_break_replays_identically() {
    fn diamond_state() -> GameStateBuilder {
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(location(1).connections([LocationId(2), LocationId(3)]))
            .with_location(location(2).connections([LocationId(1), HERE]))
            .with_location(location(3).connections([LocationId(1), HERE]))
            .with_location(location(HERE).connections([LocationId(2), LocationId(3)]))
            .with_seat(seat(1).at(HERE))
            .with_enemy(enemy(1).hunter().at(LocationId(1)))
            .open_turn(InvestigatorId(1))
    }

    // The two tied first-steps toward D are B(2) and C(3); take C, by the
    // location its option anchors to. Each run applies the same action log.
    let run = || {
        diamond_state()
            .session()
            .take(&TurnAction::EndTurn)
            .pick(OptionTarget::Location(LocationId(3)))
            .state()
            .clone()
    };
    let s1 = run();
    let s2 = run();
    // Replay determinism is a whole-state property: replaying an
    // identical action log reproduces state bit-for-bit.
    assert_eq!(
        s1, s2,
        "replaying the same action log must reproduce identical state",
    );
    assert_eq!(
        s1.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
}
