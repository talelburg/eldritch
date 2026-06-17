//! Interactive-choice resolution (Axis A, #334): the
//! [`Continuation::Choice`](crate::state::Continuation::Choice) frame's
//! suspend/resume and the uniform `0 ⇒ reject · 1 ⇒ auto · 2+ ⇒ suspend`
//! resolver (umbrella §3.4 / spec §5).
//!
//! A choice suspends by pushing a [`ChoiceFrame`] holding the picks made so
//! far (`decisions`), the offered option ids, and the root effect being
//! resolved. On resume, [`resume_choice`] validates the pick, appends it, and
//! re-runs the effect from the top — the evaluator replays `decisions` to
//! reach the next un-ground choice (single-pass suspend-and-replay).

use crate::action::InputResponse;
use crate::engine::evaluator::{apply_effect_with_decisions, EvalContext};
use crate::engine::{ChoiceOption, Cx, EngineOutcome, InputRequest, OptionId, ResumeToken};
use crate::state::{ChoiceFrame, Continuation};

/// Outcome of applying the uniform resolve convention to a count of legal
/// options (umbrella §3.4 / spec §5).
pub(crate) enum ChoiceResolution {
    /// Zero legal options — caller applies its printed fallback or rejects.
    Empty,
    /// Exactly one — auto-bind this index, no input.
    Auto(usize),
    /// Two or more — suspend with a [`Continuation::Choice`] frame.
    Suspend,
}

/// Map a legal-option count to the resolve convention.
pub(crate) fn resolve_choice_count(n: usize) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 => ChoiceResolution::Auto(0),
        _ => ChoiceResolution::Suspend,
    }
}

/// Push a [`Continuation::Choice`] frame and return the matching
/// `AwaitingInput`. `labels` provides one render label per offered option, in
/// offered order; `OptionId(i)` is the index. `decisions` carries the picks
/// already made before this suspend (so resume replays them); `effect` is the
/// root effect being resolved.
pub(crate) fn suspend_for_choice(
    cx: &mut Cx,
    prompt: impl Into<String>,
    labels: Vec<String>,
    decisions: Vec<OptionId>,
    effect: card_dsl::dsl::Effect,
    eval_ctx: EvalContext,
) -> EngineOutcome {
    let options: Vec<ChoiceOption> = labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| ChoiceOption {
            id: OptionId(u32::try_from(i).expect("offered option count fits in u32")),
            label,
        })
        .collect();
    let offered: Vec<OptionId> = options.iter().map(|o| o.id).collect();
    cx.state
        .continuations
        .push(Continuation::Choice(ChoiceFrame {
            decisions,
            offered,
            effect,
            controller: eval_ctx.controller,
            source: eval_ctx.source,
        }));
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// Suspend a card-local native leaf for a controller pick (Axis A): the frame
/// records the native effect itself as its root, so resume re-invokes the
/// native with the pick threaded via [`EvalContext::chosen_option`]. `decisions`
/// is empty — a native pick must be standalone (the native-standalone guard in
/// the evaluator enforces this). The native re-enumerates its candidates and
/// indexes by the picked [`OptionId`].
pub fn suspend_for_native_choice(
    cx: &mut Cx,
    prompt: impl Into<String>,
    labels: Vec<String>,
    tag: &str,
    ctx: &EvalContext,
) -> EngineOutcome {
    suspend_for_choice(
        cx,
        prompt,
        labels,
        Vec::new(),
        card_dsl::dsl::native(tag),
        *ctx,
    )
}

/// Resume a [`Continuation::Choice`]: validate the pick is in the offered
/// set, append it to `decisions`, pop the frame, rebuild the
/// [`EvalContext`] from the stored ingredients, and re-run the effect from
/// the top (the evaluator replays `decisions`).
pub(crate) fn resume_choice(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let InputResponse::PickSingle(picked) = response else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: a choice is open; expected InputResponse::PickSingle".into(),
        };
    };
    let Some(Continuation::Choice(frame)) = cx.state.continuations.last() else {
        return EngineOutcome::Rejected {
            reason: "resume_choice: no Choice frame on top of the stack".into(),
        };
    };
    if !frame.offered.contains(picked) {
        return EngineOutcome::Rejected {
            reason: format!("ResolveInput: PickSingle({picked:?}) not in the offered set").into(),
        };
    }
    // Pop the frame; carry forward the recorded decisions + the just-made pick.
    let Some(Continuation::Choice(frame)) = cx.state.continuations.pop() else {
        unreachable!("checked Choice on top immediately above");
    };
    let mut decisions = frame.decisions;
    decisions.push(*picked);
    let eval_ctx = match frame.source {
        Some(src) => EvalContext::for_controller_with_source(frame.controller, src),
        None => EvalContext::for_controller(frame.controller),
    };
    let outcome = apply_effect_with_decisions(cx, &frame.effect, eval_ctx, decisions);

    // If the choice completed an effect that was suspended *inside* a skill
    // test (Crypt Chill 01167's on_fail discard), re-enter the driver to run
    // the test's teardown — its continuation is parked at `PostFollowUp`.
    // Mirrors `resume_clue_interrupt`. A still-suspended outcome (a further
    // nested choice) returns as-is.
    if matches!(outcome, EngineOutcome::Done) && cx.state.in_flight_skill_test.is_some() {
        return super::skill_test::drive_skill_test(cx);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_zero_options_is_reject() {
        assert!(matches!(resolve_choice_count(0), ChoiceResolution::Empty));
    }

    #[test]
    fn resolve_one_option_auto_binds_index_zero() {
        assert!(matches!(resolve_choice_count(1), ChoiceResolution::Auto(0)));
    }

    #[test]
    fn resolve_two_options_suspends() {
        assert!(matches!(resolve_choice_count(2), ChoiceResolution::Suspend));
    }
}
