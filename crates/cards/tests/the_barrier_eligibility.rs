//! #470: The Barrier's round-end advance is gated by an eligibility predicate
//! that must reject when the Hallway group can't afford the act's clue
//! threshold. Exercises the predicate through the installed `cards::REGISTRY` —
//! the same `native_eligibility_for("01109:can_advance")` lookup the reaction
//! scan performs in `scan_act_agenda_reactions`.

use game_core::card_registry;
use game_core::state::{Act, CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{test_investigator, test_location};
use game_core::EvalContext;

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(cards::REGISTRY);
}

#[test]
fn barrier_advance_eligibility_gates_on_hallway_affordability() {
    let reg = card_registry::current().expect("registry installed");
    let pred = (reg.native_eligibility_for)("01109:can_advance")
        .expect("01109:can_advance is registered by The Barrier");

    // The Barrier's contributor location is the Hallway (01112).
    let mut hall = test_location(1, "Hallway");
    hall.code = CardCode("01112".into());
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(hall)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.act_deck = vec![Act {
        code: CardCode("01109".into()),
        clue_threshold: 3,
        resolution: None,
    }];
    state.act_index = 0;

    let ctx = EvalContext::for_controller(InvestigatorId(1));

    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .clues = 0;
    assert!(
        !pred(&state, &ctx),
        "#470: not offered at 0/3 clues (Hallway group can't afford)"
    );

    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .clues = 3;
    assert!(pred(&state, &ctx), "offered at 3/3 clues");
}
