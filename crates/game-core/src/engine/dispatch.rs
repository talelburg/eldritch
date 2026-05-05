//! Per-action dispatch handlers.
//!
//! Each function applies a single action variant to the state, mutating
//! the state in place and pushing the resulting events onto the events
//! buffer. Returns the [`EngineOutcome`] for the action.
//!
//! Handlers are split by `Action` bucket: [`apply_player_action`] for
//! human-initiated actions, [`apply_engine_record`] for engine-emitted
//! ones.

use crate::action::{EngineRecord, PlayerAction};
use crate::event::Event;
use crate::state::GameState;

use super::outcome::EngineOutcome;

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// Phase-1 minimal coverage: [`StartScenario`](PlayerAction::StartScenario)
/// and [`EndTurn`](PlayerAction::EndTurn) are implemented end-to-end;
/// other variants return [`EngineOutcome::Rejected`] with a TODO message
/// so callers and tests get a useful signal rather than a silent no-op.
pub fn apply_player_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    action: &PlayerAction,
) -> EngineOutcome {
    match action {
        PlayerAction::StartScenario => start_scenario(state, events),
        PlayerAction::EndTurn => end_turn(state, events),
        PlayerAction::ResolveInput { .. } => EngineOutcome::Rejected {
            reason: "TODO(phase-1): ResolveInput dispatch lands with the test \
                     harness in PRs #18-#20; no AwaitingInput sites exist yet."
                .into(),
        },
    }
}

/// Apply an [`EngineRecord`] to the state, pushing events.
///
/// Phase-1: all variants return [`EngineOutcome::Rejected`] with a TODO
/// message. Engine-recorded actions only flow once the RNG and skill-test
/// machinery exist (issues #16, #3 in the plan).
pub fn apply_engine_record(
    _state: &mut GameState,
    _events: &mut Vec<Event>,
    record: &EngineRecord,
) -> EngineOutcome {
    let reason = match record {
        EngineRecord::ChaosTokenDrawn { .. } => {
            "TODO(#16): ChaosTokenDrawn dispatch lands with the deterministic RNG."
        }
        EngineRecord::DeckShuffled { .. } => {
            "TODO(#16): DeckShuffled dispatch lands with the deterministic RNG."
        }
    };
    EngineOutcome::Rejected {
        reason: reason.into(),
    }
}

fn start_scenario(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    // For Phase 1 the GameState constructor places the world in its
    // initial shape; this action is the explicit "session has begun"
    // marker that lands in the action log. It also sets the round
    // counter to 1 if it isn't already, in case the constructor left it
    // at zero for a default-built state.
    if state.round == 0 {
        state.round = 1;
    }
    events.push(Event::ScenarioStarted);
    EngineOutcome::Done
}

fn end_turn(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let Some(active_id) = state.active_investigator else {
        return EngineOutcome::Rejected {
            reason: "EndTurn requires an active investigator (Investigation phase)".into(),
        };
    };
    let Some(active) = state.investigators.get_mut(&active_id) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "EndTurn: active investigator {active_id:?} is not in the investigators map"
            ),
        };
    };

    // Drain remaining actions and announce the turn ended. The phase
    // machine + turn rotation lands in #17; for now EndTurn ends only
    // the active investigator's turn without advancing phase.
    if active.actions_remaining != 0 {
        active.actions_remaining = 0;
        events.push(Event::ActionsRemainingChanged {
            investigator: active_id,
            new_count: 0,
        });
    }
    events.push(Event::TurnEnded {
        investigator: active_id,
    });
    EngineOutcome::Done
}
