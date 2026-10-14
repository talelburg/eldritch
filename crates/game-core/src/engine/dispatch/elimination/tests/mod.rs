use super::*;
use crate::state::{Continuation, GameStateBuilder};
use crate::{assert_event, assert_no_event, test_support};

mod defeat;
mod elimination_steps;
mod turn_end;

/// Two investigators mid-`Investigation`, with `whose` holding the open
/// turn. `dying` is the one about to be defeated.
fn two_investigator_open_turn(whose: InvestigatorId) -> GameState {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1).actions(2))
        .with_seat(test_support::seat(2).actions(2))
        .with_turn_order([a, b])
        .open_turn(whose)
        .build()
}
