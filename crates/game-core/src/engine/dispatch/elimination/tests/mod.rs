use super::*;
use crate::state::{Continuation, GameStateBuilder, InvestigationPhaseFrame, InvestigationResume};
use crate::{assert_event, assert_no_event, test_support};

mod defeat;
mod elimination_steps;
mod turn_end;

/// Two investigators mid-`Investigation`, with `whose` holding the open
/// turn. `dying` is the one about to be defeated.
fn two_investigator_open_turn(whose: InvestigatorId) -> GameState {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut first = test_support::test_investigator(1);
    first.actions_remaining = 2;
    let mut second = test_support::test_investigator(2);
    second.actions_remaining = 2;
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(first)
        .with_investigator(second)
        .with_active_investigator(whose)
        .with_turn_order([a, b])
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(whose)
        .build()
}
