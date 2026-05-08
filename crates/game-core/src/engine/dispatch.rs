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
use crate::state::{ChaosToken, GameState, InvestigatorId, Phase};

use super::outcome::EngineOutcome;

/// Action points granted to an investigator at the start of their
/// turn during the Investigation phase. Per the Arkham Horror LCG
/// rulebook.
const ACTIONS_PER_TURN: u8 = 3;

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
    // Round 1 begins at Mythos. We emit the entry event explicitly here
    // (rather than letting `step_phase` do it) because there is no
    // "previous" phase to emit a `PhaseEnded` for.
    state.round = 1;
    state.phase = Phase::Mythos;
    events.push(Event::ScenarioStarted);
    events.push(Event::PhaseStarted {
        phase: Phase::Mythos,
    });

    // Phase 1: Mythos / Enemy / Upkeep have no content yet, so we
    // tick straight through them. Once the engine grows real Mythos
    // draws and Enemy attacks, these phases stop being free skips.
    step_phase(state, events); // Mythos → Investigation
    if let Some(&first) = state.turn_order.first() {
        rotate_to_active(state, events, first);
    }
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

    // Drain remaining actions and announce the turn ended.
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

    // If there's another investigator after this one in turn order,
    // rotate. Otherwise the Investigation phase ends and we tick
    // through the rest of the round automatically (Phase 1: empty
    // Enemy/Upkeep/Mythos), arriving back at Investigation with the
    // first investigator active.
    let next = state
        .turn_order
        .iter()
        .position(|id| *id == active_id)
        .and_then(|idx| state.turn_order.get(idx + 1).copied());

    if let Some(next_id) = next {
        rotate_to_active(state, events, next_id);
    } else {
        state.active_investigator = None;
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos (round bumps)
        step_phase(state, events); // Mythos → Investigation
        if let Some(&first) = state.turn_order.first() {
            rotate_to_active(state, events, first);
        }
    }

    EngineOutcome::Done
}

/// Transition to the next phase: emit `PhaseEnded` for the current
/// phase, advance, emit `PhaseStarted` for the new one. Bumps the
/// round counter when entering [`Phase::Mythos`] (which is the start
/// of a new round).
///
/// **Round-bump invariant:** this is the only path that bumps
/// `state.round` post-`StartScenario`. A future caller that wants to
/// step phases for a non-round-cycle reason (e.g. a scenario effect
/// that skips a phase) will need to suppress the bump here, or the
/// round counter will drift. Revisit when such a use case appears.
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();
    events.push(Event::PhaseEnded { phase: from });
    state.phase = to;
    if to == Phase::Mythos {
        state.round += 1;
    }
    events.push(Event::PhaseStarted { phase: to });
}

/// Set `active_investigator` to `id` and refresh that investigator's
/// action points to the per-turn cap (3). Emits `ActionsRemainingChanged`.
///
/// `id` must refer to an investigator in `state.investigators` —
/// callers that pass an id from `state.turn_order` are guaranteed
/// this by the whole-program invariant "every id in `turn_order`
/// exists in `investigators`." A missing entry would be state
/// corruption, not a normal error.
fn rotate_to_active(state: &mut GameState, events: &mut Vec<Event>, id: InvestigatorId) {
    state.active_investigator = Some(id);
    let inv = state.investigators.get_mut(&id).unwrap_or_else(|| {
        unreachable!(
            "rotate_to_active: investigator {id:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    inv.actions_remaining = ACTIONS_PER_TURN;
    events.push(Event::ActionsRemainingChanged {
        investigator: id,
        new_count: ACTIONS_PER_TURN,
    });
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
