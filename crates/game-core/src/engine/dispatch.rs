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
use crate::dsl::{discover_clue, LocationTarget};
use crate::event::{Event, FailureReason};
use crate::state::{resolve_token, GameState, InvestigatorId, Phase, SkillKind, TokenResolution};

use super::evaluator::{apply_effect, EvalContext};
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
        PlayerAction::PerformSkillTest {
            investigator,
            skill,
            difficulty,
        } => perform_skill_test(state, events, *investigator, *skill, *difficulty),
        PlayerAction::Investigate { investigator } => investigate(state, events, *investigator),
        PlayerAction::ResolveInput { .. } => EngineOutcome::Rejected {
            reason: "TODO(#63): ResolveInput dispatch lands with the skill-test commit \
                     window; no AwaitingInput sites exist yet."
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
    let _ = (state, events);
    match record {
        EngineRecord::DeckShuffled { .. } => EngineOutcome::Rejected {
            reason: "TODO(#62): DeckShuffled dispatch lands when decks exist".into(),
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

/// The outcome of resolving a skill test, when the test runs at all.
/// Returned by [`resolve_skill_test`] so callers (the
/// `PerformSkillTest` dispatch wrapper, `Investigate`, future Fight /
/// Evade) can branch on success vs failure to apply action-specific
/// follow-on effects.
///
/// `Succeeded.margin` and `Failed.{reason, by}` carry the same numbers
/// as the corresponding events; callers that don't need them can
/// match `Ok(SkillTestResolution::Succeeded { .. })`. Fields are
/// `allow(dead_code)` until Fight/Evade (which want fail-by-X logic)
/// land.
#[allow(dead_code)]
pub(super) enum SkillTestResolution {
    /// The investigator's clamped total met or exceeded the difficulty.
    Succeeded {
        /// `total - difficulty` (always `>= 0`).
        margin: i8,
    },
    /// The test failed.
    Failed {
        /// Why it failed.
        reason: FailureReason,
        /// Margin of failure (always `>= 0`).
        by: i8,
    },
}

/// Run the skill-test resolution sequence and return the outcome to
/// the caller. Pushes the bracketing `SkillTestStarted` / `…Ended`
/// events plus the per-step events (`ChaosTokenRevealed`,
/// `SkillTestSucceeded` or `SkillTestFailed`).
///
/// Returns `Err(EngineOutcome::Rejected { .. })` on validation failure
/// without pushing any events.
///
/// **`AutoFail`** forces the investigator's total to 0 per the Rules
/// Reference; **`ElderSign`** is treated as `Modifier(0)` until per-
/// investigator ability dispatch lands; **negative** `skill + modifier`
/// clamps to 0.
pub(super) fn resolve_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> Result<SkillTestResolution, EngineOutcome> {
    // Validate-first: investigator must exist; chaos bag must be
    // non-empty so we can draw; difficulty must be non-negative (FFG
    // difficulties are always ≥ 0).
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("skill test: investigator {investigator:?} not in state").into(),
        });
    };
    if state.chaos_bag.tokens.is_empty() {
        return Err(EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        });
    }
    if difficulty < 0 {
        return Err(EngineOutcome::Rejected {
            reason: format!("skill test: difficulty {difficulty} must be >= 0").into(),
        });
    }
    let skill_value = inv.skills.value(skill);

    // Mutate-second: advance RNG, derive token, emit events.
    events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    let idx = state.rng.next_index(state.chaos_bag.tokens.len());
    let token = state.chaos_bag.tokens[idx];
    let resolution = resolve_token(token, &state.token_modifiers);
    events.push(Event::ChaosTokenRevealed { token, resolution });

    // All arithmetic stays in i8 with saturating ops: realistic
    // gameplay values (skill 1–8, modifier ±8, difficulty ≤ ~6) fit
    // far inside i8, but saturation defends against absurd state
    // configurations without needing a wider integer type.
    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let outcome = if margin >= 0 && fail_reason.is_none() {
        events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
        SkillTestResolution::Succeeded { margin }
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        let by = difficulty.saturating_sub(total);
        events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason,
            by,
        });
        SkillTestResolution::Failed { reason, by }
    };

    events.push(Event::SkillTestEnded { investigator });
    Ok(outcome)
}

/// Public dispatch wrapper for [`PlayerAction::PerformSkillTest`].
///
/// Card commits, the commit-window `AwaitingInput`, and the after-
/// resolution trigger window are downstream (#63 / #64). The skill-
/// test machinery itself lives in [`resolve_skill_test`], which other
/// turn-actions (Investigate, future Fight / Evade) invoke directly.
fn perform_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    match resolve_skill_test(state, events, investigator, skill, difficulty) {
        Ok(_) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
}

/// Handler for [`PlayerAction::Investigate`].
///
/// Spends 1 action, runs an intellect skill test against the location's
/// shroud, and on success applies [`Effect::DiscoverClue`] to move 1
/// clue from the location to the investigator. The discover-clue
/// evaluator handles the location-empty edge case as a silent no-op,
/// so an investigation at a 0-clue location costs the action and runs
/// the test but yields nothing — consistent with the rules.
///
/// Card-derived investigate variants (Rite of Seeking's "Action:
/// Investigate using willpower instead of intellect", Working a
/// Hunch's discover-without-test) implement their own paths; this
/// handler is the bare turn-action.
///
/// [`Effect::DiscoverClue`]: crate::dsl::Effect::DiscoverClue
fn investigate(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Validate-first.
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: investigator {investigator:?} not in state").into(),
        };
    };
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Investigate requires at least 1 action point".into(),
        };
    }
    let Some(location_id) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: {investigator:?} has no current_location to investigate")
                .into(),
        };
    };
    let Some(location) = state.locations.get(&location_id) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: location {location_id:?} (investigator's current_location) is not in state"
            )
            .into(),
        };
    };
    // Shroud is u8 in state but skill-test difficulty is i8. Saturate
    // at i8::MAX for the absurd case; realistic shrouds are 0–6.
    let difficulty = i8::try_from(location.shroud).unwrap_or(i8::MAX);

    // Mutate-second: spend the action, then resolve the test.
    let new_actions = inv.actions_remaining - 1;
    state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked above")
        .actions_remaining = new_actions;
    events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count: new_actions,
    });

    match resolve_skill_test(
        state,
        events,
        investigator,
        SkillKind::Intellect,
        difficulty,
    ) {
        Ok(SkillTestResolution::Succeeded { .. }) => {
            let effect = discover_clue(LocationTarget::ControllerLocation, 1);
            let ctx = EvalContext::for_controller(investigator);
            // discover_clue's evaluator handles empty-location as a
            // silent no-op; any rejection here would indicate the
            // investigator is between locations, which we already
            // validated. Treat any unexpected rejection as a hard
            // engine error rather than a silent failure.
            let outcome = apply_effect(state, events, &effect, ctx);
            if let EngineOutcome::Rejected { reason } = outcome {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "Investigate: discover_clue effect rejected unexpectedly: {reason}"
                    )
                    .into(),
                };
            }
            EngineOutcome::Done
        }
        Ok(SkillTestResolution::Failed { .. }) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
}
