use super::*;
use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::outcome::{EngineOutcome, OptionId};
use crate::state::{
    Act, CardCode, CardInstanceId, ChaosBag, ChaosToken, Enemy, GameStateBuilder,
    InvestigationResume,
};
use crate::{engine, test_support};

mod act_actions;
mod basic_actions;
mod combat_actions;
mod target;

/// Build a single-investigator open-turn state (`InvestigatorTurn` frame on
/// top of the `InvestigationPhase` anchor), the shape `legal_actions` enumerates.
fn open_turn_state() -> GameState {
    GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_turn_order([InvestigatorId(1)])
        // A realistic board has a non-empty chaos bag — skill-test-initiating
        // actions (Investigate) reject on an empty bag (a malformed-state
        // guard the enumerator does not replicate; real bags are never empty).
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_phase_anchor(Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(InvestigatorId(1))
        .build()
}
