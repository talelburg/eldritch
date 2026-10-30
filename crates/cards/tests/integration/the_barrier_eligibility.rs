//! Regression for #470: The Barrier's round-end advance is gated by an
//! eligibility condition that must reject when the Hallway group can't afford
//! the act's clue threshold.
//!
//! Exercises the predicate through the real `cards::REGISTRY` — the same
//! `native_condition_for("01109:can-advance")` lookup the reaction scan
//! performs through the initiation gate (`trigger_scan::collect_reactions`).
use cards::TEST_REGISTRIES;
use game_core::engine::evaluator::EvalContext;
use game_core::state::{Act, CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support;
use game_core::test_support::seat;

const INV: InvestigatorId = InvestigatorId(1);
const HALLWAY: LocationId = LocationId(1);

/// Act 01109 current, with `INV` holding `clues` in the Hallway.
fn board(clues: u8) -> GameState {
    // The Barrier's contributor location is the Hallway (01112).
    let mut hall = test_support::test_location(HALLWAY.0, "Hallway");
    hall.code = CardCode("01112".into());
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(hall)
        .with_seat(seat(1).at(HALLWAY).clues(clues))
        .with_seating([INV])
        .build();
    state.act_deck = vec![Act {
        code: CardCode("01109".into()),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    state
}

#[test]
fn barrier_advance_eligibility_gates_on_hallway_affordability() {
    let reg = TEST_REGISTRIES.cards;
    let pred = (reg.native_condition_for)("01109:can-advance")
        .expect("01109:can-advance is registered by The Barrier");
    let ctx = EvalContext::for_controller(INV);

    assert!(
        !pred(&board(0), &ctx),
        "#470: not offered at 0/3 clues (Hallway group can't afford)"
    );
    assert!(pred(&board(3), &ctx), "offered at 3/3 clues");
}
