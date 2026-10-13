//! #470: The Barrier's round-end advance is gated by an eligibility condition
//! that must reject when the Hallway group can't afford the act's clue
//! threshold. Exercises the predicate through the installed `cards::REGISTRY` —
//! the same `native_condition_for("01109:can-advance")` lookup the reaction
//! scan performs through the initiation gate (`trigger_scan::collect_reactions`).
use cards::REGISTRY;
use game_core::engine::evaluator::EvalContext;
use game_core::state::{Act, CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::seat;
use game_core::{card_registry, test_support};

const INV: InvestigatorId = InvestigatorId(1);
const HALLWAY: LocationId = LocationId(1);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Act 01109 current, with `INV` holding `clues` in the Hallway.
fn board(clues: u8) -> GameState {
    // The Barrier's contributor location is the Hallway (01112).
    let mut hall = test_support::test_location(HALLWAY.0, "Hallway");
    hall.code = CardCode("01112".into());
    let mut state = GameStateBuilder::new()
        .with_location(hall)
        .with_seat(seat(1).at(HALLWAY).clues(clues))
        .with_turn_order([INV])
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
    let reg = card_registry::current().expect("registry installed");
    let pred = (reg.native_condition_for)("01109:can-advance")
        .expect("01109:can-advance is registered by The Barrier");
    let ctx = EvalContext::for_controller(INV);

    assert!(
        !pred(&board(0), &ctx),
        "#470: not offered at 0/3 clues (Hallway group can't afford)"
    );
    assert!(pred(&board(3), &ctx), "offered at 3/3 clues");
}
