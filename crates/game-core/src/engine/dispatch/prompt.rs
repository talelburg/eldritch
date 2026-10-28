//! The prompt table: what a prompt frame asks, read from the frame.
//!
//! Every [`InputRequest`] the engine surfaces is built here, from the frame on
//! top of the continuation stack, by [`request`] and the per-kind helpers it
//! calls. Nothing else assembles one; the one request built away from a frame
//! is a native's own pick, which `NativeCx::suspend` builds through the same
//! helper ([`choice::effect_request`]) the table reads a stored pick back with.
//! A step that suspends leaves its frame at
//! rest on top and returns [`suspend`], and a frame pushed by a step that then
//! returns `Done` is surfaced the same way when the `drive` loop meets it. So
//! every prompt can be shown again from the state alone: a state settled with
//! any prompt frame on top shows its prompt, and a native whose exit pushes a
//! framework prompt (a spawn's engagement tie) just returns, the prompt
//! surfacing after it.
//!
//! The table decides only *what* is asked. *Whether* a frame rests is its drive
//! arm's call, because some frames do work before they rest: a window sweeps
//! its lapsed candidates, a deal of damage assigns its uncontested points, a
//! hand-discard cost pays a lone candidate. Those arms do their writes, then
//! return [`suspend`].

use crate::card_registry::CardRegistry;
use crate::engine::dispatch::actions::take;
use crate::engine::dispatch::{
    advance_reverse, cards, choice, combat, encounter, forced_triggers, hand_discard_cost, hunters,
    phases, reaction_windows, skill_test, slots,
};
use crate::engine::enumerate;
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::state::{
    AttackLoopFrame, AttackLoopStage, Continuation, DealDamageFrame, DealDamageStep, EffectFrame,
    FastWindowFrame, GameState, InvestigatorId, InvestigatorTurnFrame, SkillTestStep,
    TimingPointWindowFrame,
};

/// What `frame` asks when it rests on top of `state`, or `None` when it asks
/// nothing: a Driven or Inert frame, or a Prompt-profile frame at a step that
/// is not a question (a skill test between its commit window and its result
/// pause, an effect node not yet stepped).
///
/// One exhaustive arm per [`Continuation`] kind with no wildcard, so a new
/// frame kind does not compile until it says what it asks. It sits beside the
/// frame's [profile](Continuation::profile), which says whether the frame may
/// rest at all; this says what it asks when it does.
///
/// Reads `state` as well as the frame where the question depends on the board:
/// the turn menu's legal actions, a window's resolution options, the skill-test
/// under a substitution prompt.
pub(crate) fn request(
    frame: &Continuation,
    state: &GameState,
    cards: &CardRegistry,
) -> Option<InputRequest> {
    match frame {
        Continuation::InvestigatorTurn(InvestigatorTurnFrame {
            investigator,
            ending,
        }) => (!ending).then(|| turn_menu(state, cards, *investigator)),
        // An empty event window or forced run is closed on sight, never shown.
        Continuation::TimingPointWindow(TimingPointWindowFrame { candidates, .. }) => {
            if candidates.is_empty() {
                None
            } else {
                Some(reaction_windows::resolution_request(
                    state, cards, frame, candidates,
                ))
            }
        }
        // A framework Fast window holding reaction candidates resolves them like
        // any window; without, it offers the eligible fast plays.
        Continuation::FastWindow(FastWindowFrame { candidates, .. }) => {
            Some(if candidates.is_empty() {
                reaction_windows::fast_window_request(state, cards)
            } else {
                reaction_windows::resolution_request(state, cards, frame, candidates)
            })
        }
        Continuation::AdvanceReverse(advance) => advance_reverse::ack_request(advance),
        Continuation::AcknowledgeForced(acknowledge) => {
            Some(forced_triggers::acknowledge_request(cards, acknowledge))
        }
        Continuation::SkillTest(test) => match test.continuation {
            SkillTestStep::AwaitingCommit => Some(skill_test::commit_request(state, cards)),
            SkillTestStep::AcknowledgeOutcome if state.interactive_acknowledge => {
                Some(skill_test::acknowledge_request())
            }
            _ => None,
        },
        Continuation::SubstitutionPrompt(substitution) => {
            Some(skill_test::substitution_request(state, substitution))
        }
        Continuation::HunterMove(choice) => Some(hunters::choice_request(state, cards, choice)),
        Continuation::SpawnEngage(pending) => {
            Some(encounter::spawn_engage_request(state, cards, pending))
        }
        Continuation::HandSizeDiscard(queue) => {
            Some(phases::hand_size_discard_request(state, cards, queue))
        }
        Continuation::Mulligan(_) => Some(cards::mulligan_request()),
        Continuation::EncounterDraw(_) => Some(encounter::encounter_draw_request(state)),
        Continuation::SlotDiscard(slot) => Some(slots::slot_discard_request(state, cards, slot)),
        Continuation::AdditionalActionChoice(pending) => {
            Some(take::additional_action_request(state, cards, pending))
        }
        Continuation::HandDiscardCost(cost) => Some(hand_discard_cost::request(state, cards, cost)),
        Continuation::AttackLoop(AttackLoopFrame {
            investigator,
            remaining_attackers,
            stage,
            ..
        }) => match stage {
            AttackLoopStage::PickOrder => Some(combat::order_pick_request(
                state,
                cards,
                *investigator,
                remaining_attackers,
            )),
            AttackLoopStage::Attacking => None,
        },
        Continuation::DealDamage(deal @ DealDamageFrame { step, .. }) => match step {
            DealDamageStep::Distribute { .. } => {
                Some(combat::distribution_request(state, cards, deal))
            }
            DealDamageStep::Announce | DealDamageStep::Finish => None,
        },
        Continuation::Effect(
            EffectFrame::Leaf { prompt, .. } | EffectFrame::Designated { prompt, .. },
        ) => prompt.as_ref().map(choice::effect_request),
        Continuation::Effect(EffectFrame::Seq { .. } | EffectFrame::ForEach { .. })
        | Continuation::EmitEvent(_)
        | Continuation::TimingPoint(_)
        | Continuation::PlayerDraw(_)
        | Continuation::EncounterCard(_)
        | Continuation::PlayFromHand(_)
        | Continuation::MoveEnter(_)
        | Continuation::ActionResolution(_)
        | Continuation::Elimination(_)
        | Continuation::ScenarioEnd(_)
        | Continuation::MythosPhase(_)
        | Continuation::InvestigationPhase(_)
        | Continuation::EnemyPhase(_)
        | Continuation::UpkeepPhase(_) => None,
    }
}

/// Suspend on the frame on top of `state`: the `AwaitingInput` carrying what
/// it asks ([`request`]). The one way the engine suspends — a step that rests
/// its frame on top returns this.
///
/// # Panics
///
/// If the stack is empty or its top frame asks nothing. Either is an engine
/// bug: the caller suspended on a frame that is not at rest as a prompt.
pub(crate) fn suspend(state: &GameState, cards: &CardRegistry) -> EngineOutcome {
    let top = state
        .continuations
        .top()
        .expect("prompt::suspend: the continuation stack is empty");
    let request = request(top, state, cards)
        .unwrap_or_else(|| panic!("prompt::suspend: the top frame asks nothing: {top:?}"));
    EngineOutcome::AwaitingInput {
        request,
        // TODO(#458): deterministic resume tokens; 0 is the placeholder every
        // `AwaitingInput` carries.
        resume_token: ResumeToken(0),
    }
}

/// The open-turn action menu: the legal-action enumeration as a `PickSingle`.
/// `OptionId(i)` indexes
/// [`legal_actions`](crate::engine::enumerate::legal_actions); the
/// `InvestigatorTurn` arm of `resolve_input` re-enumerates and dispatches.
///
/// The prompt itself is anchored to the acting investigator's turn control, so a
/// host can suppress its "Choose an action" text structurally rather than by
/// matching the string (ADR 0011).
fn turn_menu(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
) -> InputRequest {
    let options = enumerate::legal_actions(state, cards)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(state, cards),
            )
            .maybe_at(a.target(state))
        })
        .collect();
    InputRequest::pick_single("Choose an action", options)
        .at(OptionTarget::TurnControl(investigator))
}
