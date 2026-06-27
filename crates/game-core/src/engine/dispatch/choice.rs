//! Interactive-choice resolution (#422): effect nodes that need a controller
//! pick **suspend in place** — the evaluator leaves the node's
//! [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf) on top of the
//! continuation stack as the prompt. [`resume_effect_choice`] sets the pick on
//! that frame and re-steps it. The `0 ⇒ reject · 1 ⇒ auto · 2+ ⇒ suspend`
//! resolver ([`resolve_choice_count`]) is shared by the evaluator and
//! card-local natives. No replay, no separate choice frame (umbrella §3.4).

use crate::action::InputResponse;
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

/// Map a legal-option count to the resolve convention. When `interactive` is set
/// (human play, `interactive_acknowledge`), a single option surfaces as a
/// one-option pick (`Suspend`) instead of auto-binding silently (#466).
pub fn resolve_choice_count(n: usize, interactive: bool) -> ChoiceResolution {
    match n {
        0 => ChoiceResolution::Empty,
        1 if interactive => ChoiceResolution::Suspend,
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
        request: InputRequest::pick_single(prompt, options),
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
    resume_effect_walk(cx)
}

/// Resume a parked effect walk after a player input by ceding to the global
/// `drive` loop (Slice D #423). The caller ([`resume_effect_choice`] / the
/// effect-path arm of `resume_damage_assignment`, K5b-2) has already recorded
/// the input on the suspended top `Effect` leaf; returning `Done` hands the
/// parked frames to `apply_player_action`'s `drive(cx, outcome)`, whose
/// `Continuation::Effect` arm steps them via the same `step_effect_frame` the
/// old bounded `drive_effect_to_base` used, then dispatches whatever frame the
/// walk was nested within (a `SkillTest` mid-resolution, a window with remaining
/// candidates). No bounded re-entry, no reach-down.
pub(crate) fn resume_effect_walk(_cx: &mut Cx) -> EngineOutcome {
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::Effect;
    use crate::engine::evaluator::{push_effect, EvalContext};

    #[test]
    fn resolve_zero_options_is_reject() {
        assert!(matches!(
            resolve_choice_count(0, false),
            ChoiceResolution::Empty
        ));
        assert!(matches!(
            resolve_choice_count(0, true),
            ChoiceResolution::Empty
        ));
    }

    #[test]
    fn resolve_one_option_auto_binds_when_not_interactive() {
        assert!(matches!(
            resolve_choice_count(1, false),
            ChoiceResolution::Auto(0)
        ));
    }

    #[test]
    fn resolve_one_option_suspends_when_interactive() {
        // #466: a lone option surfaces as a one-option pick in human play.
        assert!(matches!(
            resolve_choice_count(1, true),
            ChoiceResolution::Suspend
        ));
    }

    #[test]
    fn resolve_two_options_suspends_regardless_of_flag() {
        assert!(matches!(
            resolve_choice_count(2, false),
            ChoiceResolution::Suspend
        ));
        assert!(matches!(
            resolve_choice_count(2, true),
            ChoiceResolution::Suspend
        ));
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
        // Push the effect root + drive it through the real global loop (the
        // deleted `apply_effect` bounded entry's test-only successor); a
        // 2-branch ChooseOne suspends in place for a pick.
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
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

    #[test]
    fn single_branch_choose_one_surfaces_under_interactive_flag() {
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;

        // One ChooseOne branch: today it auto-binds. With interactive_acknowledge
        // on it must surface as a one-option pick (#466).
        let effect = Effect::ChooseOne(vec![Effect::Seq(vec![])]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));

        let mut state = GameStateBuilder::default().build();
        state.interactive_acknowledge = true;
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(
                    request.options.len(),
                    1,
                    "lone branch surfaces as one option"
                );
            }
            other => panic!("expected a one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn single_branch_choose_one_auto_binds_when_flag_off() {
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;

        let effect = Effect::ChooseOne(vec![Effect::Seq(vec![])]);
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let mut state = GameStateBuilder::default().build(); // flag defaults false
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            push_effect(&mut cx, &effect, ctx);
            crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done)
        };
        assert!(
            matches!(out, EngineOutcome::Done),
            "flag off: auto-binds, no suspend"
        );
    }
}
