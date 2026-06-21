//! Interactive-choice resolution (#422): effect nodes that need a controller
//! pick **suspend in place** — the evaluator leaves the node's
//! [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf) on top of the
//! continuation stack as the prompt. [`resume_effect_choice`] sets the pick on
//! that frame and re-steps it. The `0 ⇒ reject · 1 ⇒ auto · 2+ ⇒ suspend`
//! resolver ([`resolve_choice_count`]) is shared by the evaluator and
//! card-local natives. No replay, no separate choice frame (umbrella §3.4).

use crate::action::InputResponse;
use crate::engine::evaluator::drive_effect_to_base;
use crate::engine::{ChoiceOption, Cx, EngineOutcome, InputRequest, OptionId, ResumeToken};
use crate::state::{Continuation, EffectFrame};

/// Outcome of applying the uniform resolve convention to a count of legal
/// options (umbrella §3.4 / spec §5). `pub` so card-local natives can apply
/// the same convention as the evaluator (Crypt Chill 01167).
pub enum ChoiceResolution {
    /// Zero legal options — caller applies its printed fallback or rejects.
    Empty,
    /// Exactly one — auto-bind this index, no input.
    Auto(usize),
    /// Two or more — suspend for a controller pick.
    Suspend,
}

/// Map a legal-option count to the resolve convention.
pub fn resolve_choice_count(n: usize) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 => ChoiceResolution::Auto(0),
        _ => ChoiceResolution::Suspend,
    }
}

/// Build the `AwaitingInput` for a controller choice from one render label per
/// offered option, in offered order (`OptionId(i)` is the index). Pushes
/// **nothing** — the suspending effect node's own `Leaf` frame stays on the
/// stack as the prompt (#422); resume re-derives the option set and validates
/// the pick by checked indexing.
pub(crate) fn awaiting_choice(prompt: impl Into<String>, labels: Vec<String>) -> EngineOutcome {
    let options: Vec<ChoiceOption> = labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| ChoiceOption {
            id: OptionId(u32::try_from(i).expect("offered option count fits in u32")),
            label,
        })
        .collect();
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// Suspend a card-local native leaf for a controller pick (#422): build the
/// `AwaitingInput` from `labels`. The native's `Leaf` frame stays on the stack;
/// resume re-invokes the native with the pick threaded via
/// [`EvalContext::chosen_option`](crate::engine::EvalContext::chosen_option).
/// The native re-enumerates its candidates and indexes by the picked
/// [`OptionId`]. (`cx`/`tag`/`ctx` are accepted for call-site compatibility; the
/// frame management lives in the evaluator's native step.)
pub fn suspend_for_native_choice(
    _cx: &mut Cx,
    prompt: impl Into<String>,
    labels: Vec<String>,
    _tag: &str,
    _ctx: &crate::engine::EvalContext,
) -> EngineOutcome {
    awaiting_choice(prompt, labels)
}

/// Resume an effect node suspended in place for a controller pick (#422): the
/// top frame is the suspended [`EffectFrame::Leaf`]. Set its `chosen_option` and
/// re-step it via the effect drive — the node grounds/picks (checked indexing,
/// validate-first) instead of suspending. On completion, re-enter the enclosing
/// driver (skill test / reaction window), mirroring the former replay resume.
pub(crate) fn resume_effect_choice(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let InputResponse::PickSingle(picked) = response else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: a choice is open; expected InputResponse::PickSingle".into(),
        };
    };
    match cx.state.continuations.last_mut() {
        Some(Continuation::Effect(EffectFrame::Leaf { ctx, .. })) => {
            ctx.set_chosen_option(Some(*picked));
        }
        _ => {
            return EngineOutcome::Rejected {
                reason: "resume_effect_choice: top frame is not a suspended effect leaf".into(),
            }
        }
    }
    // Drive the contiguous run of effect frames on top (the resumed walk) until
    // it completes or suspends again. `base` is the depth just below that run.
    let base = cx
        .state
        .continuations
        .iter()
        .rposition(|c| !matches!(c, Continuation::Effect(_)))
        .map_or(0, |i| i + 1);
    let outcome = drive_effect_to_base(cx, base);

    // If the walk completed inside a skill test (e.g. Crypt Chill 01167's
    // on_fail discard) or a reaction window (Research Librarian 01032's
    // SearchDeck), re-enter that driver so it advances / tears down — mirroring
    // the former replay resume. A still-suspended outcome returns as-is.
    if matches!(outcome, EngineOutcome::Done) {
        if cx.state.has_skill_test_in_flight() {
            return super::skill_test::drive_skill_test(cx);
        }
        if let Some(idx) = cx.state.top_reaction_window_index() {
            return super::reaction_windows::advance_resolution(cx, idx);
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::Effect;
    use crate::engine::evaluator::{apply_effect, EvalContext};

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

    #[test]
    fn suspended_leaf_snapshots_active_skill_test_binding() {
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;

        // A context carrying an active on_fail margin when a ChooseOne suspends.
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_failed_by(2);

        let effect = Effect::ChooseOne(vec![Effect::Seq(vec![]), Effect::Seq(vec![])]);
        let mut state = GameStateBuilder::default().build();
        let mut events = Vec::new();
        let out = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx,
        );
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "a 2-branch ChooseOne suspends for a pick",
        );

        let Some(Continuation::Effect(EffectFrame::Leaf { ctx, .. })) = state.continuations.last()
        else {
            panic!("expected a suspended effect Leaf frame on the stack");
        };
        assert_eq!(
            ctx.failed_by(),
            Some(2),
            "the active skill-test margin must ride the suspended Leaf frame's context, \
             not be dropped at suspend",
        );
    }
}
