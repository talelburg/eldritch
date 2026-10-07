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
//! Renamed on the way down from `crates/scenarios/tests/hunter_movement.rs`
//! (#873): the surviving test is about the tie and the `PickSingle` that settles
//! it, not about hunter movement at large. The spawn-engagement tie that also
//! lived in that file went to real cards under #877.

use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, MockRegistry};

#[ctor::ctor(unsafe)]
fn install() {
    // No probe cards: the diamond is pure topology. `install`'s composed
    // `metadata_for_test_inv` is what `test_investigator`'s capacity reads want.
    MockRegistry::new().install();
}

#[test]
fn hunter_move_tie_break_replays_identically() {
    fn diamond_state() -> GameStateBuilder {
        let mut loc_a = test_support::test_location(1, "A");
        let mut loc_b = test_support::test_location(2, "B");
        let mut loc_c = test_support::test_location(3, "C");
        let mut loc_d = test_support::test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_support::test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_support::test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        GameStateBuilder::new()
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_enemy(hunter)
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
