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
use crate::state::{ChaosToken, GameState, Phase};

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
            reason: "TODO(#18-#20): ResolveInput dispatch lands with the test \
                     harness; no AwaitingInput sites exist yet."
                .into(),
        },
    }
}

/// Apply an [`EngineRecord`] to the state, pushing events.
pub fn apply_engine_record(
    state: &mut GameState,
    events: &mut Vec<Event>,
    record: &EngineRecord,
) -> EngineOutcome {
    match record {
        EngineRecord::ChaosTokenDrawn { token } => chaos_token_drawn(state, events, *token),
        EngineRecord::DeckShuffled { .. } => EngineOutcome::Rejected {
            reason: "TODO: DeckShuffled dispatch lands when decks exist (#15+ scenario plumbing)"
                .into(),
        },
    }
}

fn start_scenario(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    // The GameState constructor places the world in its initial shape;
    // this action is the explicit "session has begun" marker that lands
    // in the action log. Replaying it on an already-started state is a
    // bug, not a no-op — reject so callers notice rather than silently
    // double-emitting `ScenarioStarted`.
    if state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "StartScenario applied to a state that is already in progress".into(),
        };
    }
    state.round = 1;
    events.push(Event::ScenarioStarted);
    EngineOutcome::Done
}

fn end_turn(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: "EndTurn is only valid during the Investigation phase".into(),
        };
    }
    let Some(active_id) = state.active_investigator else {
        return EngineOutcome::Rejected {
            reason: "EndTurn requires an active investigator".into(),
        };
    };
    // The Some(active_investigator) invariant is paired with that ID
    // existing in the investigators map; a missing entry would be state
    // corruption, not a normal rejection. Surface it loudly rather than
    // hiding behind Rejected.
    let active = state.investigators.get_mut(&active_id).unwrap_or_else(|| {
        unreachable!(
            "active_investigator {active_id:?} is not in the investigators map; \
                 this is a state-corruption invariant violation"
        )
    });

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

/// Handler for [`EngineRecord::ChaosTokenDrawn`].
///
/// Validate-first pattern: clone the RNG, draw an index, check the
/// recorded token matches `chaos_bag[index]`. Only on match do we
/// commit the RNG advance and emit the event. A mismatch indicates
/// log corruption — return `Rejected` without mutating state.
///
/// Modifier resolution (token symbols → numeric modifier per scenario)
/// lands later. For Phase 1 we emit `modifier: 0` so the event shape
/// is right; downstream consumers will pick up the real modifier when
/// scenario-aware token effects exist.
fn chaos_token_drawn(
    state: &mut GameState,
    events: &mut Vec<Event>,
    token: ChaosToken,
) -> EngineOutcome {
    if state.chaos_bag.tokens.is_empty() {
        return EngineOutcome::Rejected {
            reason: "ChaosTokenDrawn requires a non-empty chaos bag".into(),
        };
    }
    let mut probe = state.rng.clone();
    let idx = probe.next_index(state.chaos_bag.tokens.len());
    let derived = state.chaos_bag.tokens[idx];
    if derived != token {
        return EngineOutcome::Rejected {
            reason: format!(
                "ChaosTokenDrawn: recorded token {token:?} does not match RNG-derived \
                 {derived:?} (log corruption or wrong seed)"
            )
            .into(),
        };
    }
    state.rng = probe;
    events.push(Event::ChaosTokenRevealed { token, modifier: 0 });
    EngineOutcome::Done
}
