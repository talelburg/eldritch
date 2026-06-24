//! Skill-test resolution handlers.
//!
//! Contains the full skill-test lifecycle: starting a test
//! ([`start_skill_test`]), the commit-stage entry ([`finish_skill_test`]),
//! the resolution driver ([`advance`]), and all supporting
//! helpers.

use std::collections::{BTreeMap, BTreeSet};

use crate::card_registry;
use crate::dsl::{discover_clue, LocationTarget, SkillTestKind, Trigger};
use crate::event::{Event, FailureReason};
use crate::state::{
    resolve_token, CardCode, ChaosToken, Continuation, GameState, InFlightSkillTest,
    InvestigatorId, SkillKind, SkillTestFollowUp, SkillTestStep, Status, TokenResolution, Zone,
};

use super::super::evaluator::{
    constant_skill_modifier, pending_skill_modifier, push_effect, EvalContext,
};
use super::super::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use super::Cx;
use crate::action::InputResponse;

// Nine args: the skill-test parameters are genuinely independent axes
// (skill, kind, difficulty, success/fail follow-ups, source). A params
// struct would add indirection without grouping anything cohesive.
#[allow(clippy::too_many_arguments)]
pub(in crate::engine) fn start_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty: i8,
    follow_up: SkillTestFollowUp,
    on_success: Option<card_dsl::dsl::Effect>,
    on_fail: Option<card_dsl::dsl::Effect>,
    source: Option<crate::state::CardInstanceId>,
    test_modifier: i8,
) -> EngineOutcome {
    // Validate-first: investigator must exist and be Active; chaos
    // bag must be non-empty so we can draw; difficulty must be non-
    // negative (FFG difficulties are always ≥ 0). Defeated
    // investigators can't take skill tests — they're out of play.
    // A second test cannot overlap an in-flight one.
    let Some(inv) = cx.state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("skill test: investigator {investigator:?} not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill test: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if cx.state.chaos_bag.tokens.is_empty() {
        return EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        };
    }
    if difficulty < 0 {
        return EngineOutcome::Rejected {
            reason: format!("skill test: difficulty {difficulty} must be >= 0").into(),
        };
    }
    if cx.state.has_skill_test_in_flight() {
        return EngineOutcome::Rejected {
            reason: "skill test: another skill test is already in flight; only one test \
                     may pause at a commit window at a time"
                .into(),
        };
    }

    // Mutate-second: stash the in-flight record and announce the test.
    // Snapshot the investigator's location for
    // `LocationTarget::TestedLocation` resolution during
    // `Trigger::OnSkillTestResolution` firing. `inv`'s immutable
    // borrow from the validation block above is still live; reading
    // `current_location` here doesn't extend it past this line.
    let tested_location = inv.current_location;
    // Push the SkillTest frame up front (carrying the test's data — #348), so the
    // in-flight test has a home from test start. Safe: all validation precedes
    // this point, and the only outcomes below are `AwaitingInput` (the
    // Mind-over-Matter substitution prompt, or the commit window). During the
    // substitution prompt this frame sits beneath the `SubstitutionPrompt`
    // frame, which top-frame dispatch routes first.
    cx.state
        .continuations
        .push(crate::state::Continuation::SkillTest(InFlightSkillTest {
            investigator,
            skill,
            kind,
            difficulty,
            committed_by_active: Vec::new(),
            tested_location,
            follow_up,
            on_fail,
            on_success,
            source,
            continuation: SkillTestStep::PreCommitWindow,
            test_modifier,
            bonus_attack_damage: 0,
        }));
    cx.events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    // "Use X in place of Y?" (Mind over Matter 01036): if this is a Combat or
    // Agility test and the investigator has a covering round-scoped
    // substitution active, offer the choice BEFORE the commit window — the
    // test type is fixed here (per the card's FAQ). The in-flight record (just
    // created) is the parking; `resume_substitution_choice` rewrites its skill
    // on "yes". Routed via a `SubstitutionPrompt` frame above the `SkillTest`.
    if substitution_covers(cx.state, investigator, skill) {
        cx.state
            .continuations
            .push(crate::state::Continuation::SubstitutionPrompt { investigator });
        let use_skill = SkillKind::Intellect; // sole substitution in scope
        return EngineOutcome::AwaitingInput {
            request: InputRequest::choice(
                format!(
                    "{investigator:?}: use {use_skill:?} in place of {skill:?} for this test? \
                     (PickSingle(0) = use {use_skill:?}, PickSingle(1) = keep {skill:?})",
                ),
                vec![
                    ChoiceOption {
                        id: OptionId(0),
                        label: format!("Use {use_skill:?}"),
                    },
                    ChoiceOption {
                        id: OptionId(1),
                        label: format!("Keep {skill:?}"),
                    },
                ],
            ),
            resume_token: ResumeToken(0),
        };
    }
    // No substitution: drive the test. `advance` parks at `AwaitingCommit`,
    // emitting the commit prompt (which propagates up to here).
    advance(cx)
}

/// Whether `investigator` has an active round-scoped substitution covering a
/// `skill` test (Mind over Matter 01036: Intellect for Combat/Agility).
fn substitution_covers(state: &GameState, investigator: InvestigatorId, skill: SkillKind) -> bool {
    state
        .skill_substitutions
        .iter()
        .any(|s| s.investigator == investigator && s.for_skills.contains(&skill))
}

/// Resume the Mind over Matter substitution prompt (#322): `PickSingle(0)`
/// rewrites the in-flight test to an Intellect test (dropping any weapon combat
/// bonus per the FAQ "ignore bonuses to Combat or Agility"); `PickSingle(1)`
/// keeps the printed skill (a genuine "may" — a player may decline to fail on
/// purpose). Either way, parks for the `drive` loop, which opens the next window
/// (the ST.1 player window, or the commit prompt on auto-skip).
pub(in crate::engine) fn resume_substitution_choice(
    cx: &mut Cx,
    response: &InputResponse,
) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(opt)) = response else {
        return EngineOutcome::Rejected {
            reason: "substitution prompt expects PickSingle(0|1)".into(),
        };
    };
    if *opt > 1 {
        return EngineOutcome::Rejected {
            reason: format!("substitution prompt: PickSingle({opt}) out of range (0|1)").into(),
        };
    }
    // Pop the SubstitutionPrompt frame we validated against (it is the top
    // frame, above the SkillTest frame the mutation below reaches).
    cx.state.continuations.pop();
    if *opt == 0 {
        // Use Intellect: the test becomes an Intellect test (base / icons /
        // bonuses all key off `skill`), and a weapon's combat bonus
        // (`test_modifier`) is dropped — FAQ "ignore any bonuses to Combat or
        // Agility". Bonus damage (`extra_damage` / `bonus_attack_damage`) is
        // separate and untouched.
        let t = cx
            .state
            .current_skill_test_mut()
            .expect("resume_substitution_choice: in-flight test must exist");
        t.skill = SkillKind::Intellect;
        t.test_modifier = 0;
    }
    // Park: return `Done` so the `drive` loop's `SkillTest` arm drives the test
    // from its pre-commit cursor — opening the ST.1 player window (#374), then
    // (on auto-skip) the commit prompt reading the now-possibly-rewritten skill,
    // or parking at the window if a Fast play is available. The frame is on top
    // and `resolve_input`'s caller drives it. Slice C, #431 — the
    // substitution-resume `advance` reach-down is retired.
    EngineOutcome::Done
}

/// Commit-stage entry to the skill-test resolution driver. Handles the
/// response to the
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput) the engine emitted at
/// the commit window: validate the supplied indices, persist them onto the
/// in-flight record, pre-advance the cursor to [`SkillTestStep::PreTokenWindow`],
/// and return [`EngineOutcome::Done`] so the `drive` loop's `SkillTest` arm runs
/// the resolution body ([`run_resolution`]) and the remaining steps. (Slice C,
/// #431 — the commit-hop `advance` reach-down is retired.)
///
/// On invalid input (no in-flight test, malformed indices, or continuation
/// already advanced) returns [`EngineOutcome::Rejected`] with no state change
/// and no events pushed — the engine stays paused so the caller can submit a
/// fixed-up response.
pub(super) fn finish_skill_test(cx: &mut Cx, indices: &[u32]) -> EngineOutcome {
    let Some(in_flight) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: "skill-test commit: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(in_flight.continuation, SkillTestStep::AwaitingCommit) {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill-test commit: commit window already closed (continuation {:?}); \
                 the engine is mid-resolution, not at the commit step",
                in_flight.continuation,
            )
            .into(),
        };
    }
    let investigator = in_flight.investigator;

    // Validate the commit indices against the resolving
    // investigator's hand. On Err, state is untouched and the engine
    // stays paused so the client can retry.
    let indices_u8 = match validate_commit_indices(cx.state, investigator, indices) {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };

    // Persist the committed indices and pre-advance the cursor to
    // `PreTokenWindow`, then park: return `Done` so the `drive` loop's
    // `SkillTest` arm (dispatch/mod.rs) runs the resolution body from there.
    // The frame stays on top and `resolve_input`'s caller drives it
    // (apply_player_action runs `drive` after this returns). Slice C, #431 —
    // the commit-hop `advance` reach-down is retired.
    let t = cx
        .state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above");
    t.committed_by_active = indices_u8;
    t.continuation = SkillTestStep::PreTokenWindow;
    EngineOutcome::Done
}

/// Run the computation half of a skill test (RR ST.3–ST.6): sum the committed
/// icons and resolve the chaos token (emitting
/// [`SkillTestSucceeded`](crate::Event::SkillTestSucceeded) /
/// [`SkillTestFailed`](crate::Event::SkillTestFailed)). This step pushes no
/// effect — every result effect (ST.7) is deferred to the cursor-sequenced
/// [`FireOnCommit`](SkillTestStep::FireOnCommit) /
/// [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp) /
/// [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect) /
/// [`FireOnResolution`](SkillTestStep::FireOnResolution) steps that the driver
/// runs in turn — so it just pre-advances the cursor to
/// [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions) (threading
/// `succeeded`/`failed_by`) and returns; the `advance` loop reads the next step.
fn run_resolution(cx: &mut Cx, investigator: InvestigatorId, indices_u8: &[u8]) {
    let (skill, kind, difficulty) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("run_resolution: the SkillTest frame must exist");
        (t.skill, t.kind, t.difficulty)
    };

    let skill_value = sum_skill_value(cx.state, investigator, skill, kind, indices_u8);

    let (succeeded, failed_by) =
        resolve_chaos_token_and_emit(cx, investigator, skill, difficulty, skill_value);

    // Pre-advance to the EmitSuccessReactions step (the ST.6→ST.7 boundary,
    // where the "after you successfully investigate" timing point fires on the
    // success just established — before any ST.7 consequence). Nothing was
    // pushed here, so the `advance` loop stays on this SkillTest frame and
    // `continue`s into EmitSuccessReactions on the next iteration.
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::EmitSuccessReactions {
        succeeded,
        failed_by,
    };
}

/// RR ST.7 head — the [`FireOnCommit`](SkillTestStep::FireOnCommit) step.
/// Collect the committed cards' [`Trigger::OnCommit`] ability effects (Vicious
/// Blow 01025's `BoostAttackDamage`), combine them into one
/// [`Effect::Seq`](crate::dsl::Effect::Seq), and `push_effect` it for the drive
/// loop (push nothing if no committed card carries an `OnCommit` trigger).
/// Pre-advances the cursor to [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp)
/// **before** the push, so a suspending effect would resume past this step.
///
/// These effects are conditional on success ("If this skill test is successful
/// during an attack…") so they sit after the token resolves, but **before**
/// `ApplyFollowUp` reads the `bonus_attack_damage` accumulator they populate.
/// The in-scope `BoostAttackDamage` is a non-suspending stat boost, so the loop
/// drives it and re-dispatches this `SkillTest` at `ApplyFollowUp`, which reads
/// the now-populated accumulator.
fn fire_on_commit_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    indices_u8: &[u8],
    succeeded: bool,
    failed_by: u8,
) {
    let effects = collect_on_commit(cx, investigator, indices_u8);
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::ApplyFollowUp {
        succeeded,
        failed_by,
    };
    if !effects.is_empty() {
        let seq = crate::dsl::Effect::Seq(effects);
        push_effect(cx, &seq, EvalContext::for_controller(investigator));
    }
}

/// RR ST.7 part 1 — apply the action-specific [`SkillTestFollowUp`].
///
/// On **success** the follow-up runs: the Investigate follow-up pushes its
/// `discover_clue` effect (yielding to the drive loop), while Fight / Evade /
/// None mutate synchronously. On **failure** the follow-up is skipped entirely
/// (every variant is success-only). The cursor is pre-advanced to
/// [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect) **before** any push,
/// so a suspending discovery (Cover Up 01007's before-discover interrupt)
/// resumes at `ApplyResultEffect` rather than re-running the follow-up. (The
/// "after you successfully investigate" timing point already fired at the
/// preceding [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions) step,
/// before this discovery — RR ST.6 success precedes the ST.7 consequence.)
///
/// When the Investigate follow-up pushes `discover_clue` the `advance` loop's
/// top-frame check yields on the next iteration (the Effect frame is now
/// `last()`); Fight/Evade/None push nothing and the loop `continue`s on this
/// frame straight into `ApplyResultEffect`.
fn apply_follow_up_step(cx: &mut Cx, investigator: InvestigatorId, succeeded: bool, failed_by: u8) {
    let follow_up = cx
        .state
        .current_skill_test()
        .expect("apply_follow_up_step: the SkillTest frame must exist")
        .follow_up;

    // Pre-advance BEFORE any push, so a suspending discovery resumes past here.
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame was present immediately above")
        .continuation = SkillTestStep::ApplyResultEffect {
        succeeded,
        failed_by,
    };

    if succeeded {
        apply_skill_test_follow_up(cx, investigator, follow_up);
    }
}

/// RR ST.6→ST.7 boundary — the
/// [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions) step. Once
/// success has been *established* (ST.6) but **before** any ST.7 consequence
/// resolves (the clue discovery in `ApplyFollowUp`, the result effects in
/// `ApplyResultEffect`), fire the "after you successfully investigate" timing
/// point (Obscuring Fog 01168 forced + Dr. Milan 01029 reaction, one
/// `emit_event` so forced precedes reaction — RR p.2, #213). A no-op for every
/// non-Investigate follow-up and on failure. Returns the `emit_event` outcome
/// so the caller yields if a 2+ forced run suspends. Pre-advances the cursor to
/// [`FireOnCommit`](SkillTestStep::FireOnCommit) first, so a suspending reaction
/// window resumes past this step into the ST.7 consequences (which can then see
/// any state the ST.6 reactions changed).
fn emit_success_reactions_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
    failed_by: u8,
) -> EngineOutcome {
    let follow_up = {
        let t = cx
            .state
            .current_skill_test()
            .expect("emit_success_reactions_step: the SkillTest frame must persist");
        t.follow_up
    };
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::FireOnCommit {
        succeeded,
        failed_by,
    };
    if succeeded && matches!(follow_up, SkillTestFollowUp::Investigate) {
        if let Some(location) = cx
            .state
            .investigators
            .get(&investigator)
            .and_then(|i| i.current_location)
        {
            return super::emit::emit_event(
                cx,
                &super::emit::TimingEvent::SuccessfullyInvestigated {
                    investigator,
                    location,
                },
            );
        }
    }
    EngineOutcome::Done
}

/// RR ST.7 part 2 — the [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect)
/// step. Push the success/failure card effect (exactly one, or neither):
/// `on_success` on a passing draw (Frozen in Fear 01164), `on_fail` on a failing
/// draw (Crypt Chill 01167, Grasping Hands 01162). Pre-advances the cursor to
/// [`FireOnResolution`](SkillTestStep::FireOnResolution) **before** the push so a
/// suspending effect resumes past this step. The push (if any) makes an Effect
/// frame the new top → the `advance` loop yields to drive it.
fn apply_result_effect_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
    failed_by: u8,
) {
    let (on_success, on_fail, source) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("apply_result_effect_step: the SkillTest frame must persist");
        (t.on_success.clone(), t.on_fail.clone(), t.source)
    };
    cx.state
        .current_skill_test_mut()
        .expect("the SkillTest frame must persist across driver steps")
        .continuation = SkillTestStep::FireOnResolution { succeeded, next: 0 };
    // Thread `source` so `Effect::DiscardSelf` finds itself across the
    // suspend/resume boundary.
    let card_ctx =
        |inv: InvestigatorId| EvalContext::for_controller_with_optional_source(inv, source);
    if succeeded {
        if let Some(effect) = &on_success {
            push_effect(cx, effect, card_ctx(investigator));
        }
    } else if let Some(effect) = &on_fail {
        // Thread the failure margin so `Effect::ForEachPointFailed`
        // (Grasping Hands 01162) can scale.
        let mut ctx = card_ctx(investigator);
        ctx.set_failed_by(failed_by);
        push_effect(cx, effect, ctx);
    }
}

/// RR ST.7 — the [`FireOnResolution`](SkillTestStep::FireOnResolution) step.
/// Fire the committed cards' `OnSkillTestResolution` triggers, one effect per
/// visit, in committed-card order. Collect the matching effects, push the
/// `next`th (pre-advancing `next` first), and yield (the pushed Effect frame is
/// the new top); when `next` runs past the list, advance to
/// [`PostRetaliate`](SkillTestStep::PostRetaliate).
fn fire_on_resolution_step(
    cx: &mut Cx,
    investigator: InvestigatorId,
    indices_u8: &[u8],
    succeeded: bool,
    next: u32,
) {
    let effects = collect_on_skill_test_resolution(cx, investigator, indices_u8, succeeded);
    let idx = next as usize;
    if idx < effects.len() {
        cx.state
            .current_skill_test_mut()
            .expect("the SkillTest frame must persist across driver steps")
            .continuation = SkillTestStep::FireOnResolution {
            succeeded,
            next: next.saturating_add(1),
        };
        push_effect(cx, &effects[idx], EvalContext::for_controller(investigator));
    } else {
        cx.state
            .current_skill_test_mut()
            .expect("the SkillTest frame must persist across driver steps")
            .continuation = SkillTestStep::PostRetaliate { succeeded };
    }
}

/// Emit the commit-window prompt (the `AwaitingCommit` step's `awaiting()`). The
/// test parks here for the active investigator's commit; a Resolution frame
/// pushed *above* it is a mid-test window. Reached from `start_skill_test` and
/// `resume_substitution_choice` (which call `advance`), so the commit
/// `AwaitingInput` propagates up the call stack, halting any enclosing forced run
/// (the commit `ResolveInput` resumes via `finish_skill_test`).
fn emit_commit_window(cx: &Cx, investigator: InvestigatorId) -> EngineOutcome {
    let (skill, difficulty) = {
        let t = cx
            .state
            .current_skill_test()
            .expect("emit_commit_window: in-flight test must exist");
        (t.skill, t.difficulty)
    };
    EngineOutcome::AwaitingInput {
        request: InputRequest::prompt(format!(
            "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
             (difficulty {difficulty}); submit InputResponse::PickMultiple with the \
             hand indices as option ids. Empty selection commits no cards.",
        )),
        resume_token: ResumeToken(0),
    }
}

/// Open one of the RR p.26 skill-test framework player windows (#374) and return
/// `open_fast_window`'s outcome directly. Pre-advances the cursor to `next`
/// **before** opening (the suspend/resume invariant), so a resume — whether the
/// auto-skip inline `run_fast_continuation -> advance` or a wait-then-close —
/// picks up at `next`, not by re-opening this window.
///
/// The caller must **return** this outcome, never fall through: `open_fast_window`
/// returns `Done` both on auto-skip (continuation already ran) and when it parks
/// a pure-Fast window on top (an empty `FastWindow` gate the `advance` loop does
/// not re-dispatch — falling through would emit the next step's prompt *over* the
/// parked window).
fn open_skill_test_player_window(
    cx: &mut Cx,
    next: SkillTestStep,
    before_token: bool,
) -> EngineOutcome {
    cx.state
        .current_skill_test_mut()
        .expect("open_skill_test_player_window: the SkillTest frame must exist")
        .continuation = next;
    super::reaction_windows::open_fast_window(
        cx,
        crate::state::FastWindowKind::SkillTest { before_token },
    )
}

/// Walk the skill-test resolution sequence from the current
/// [`SkillTestStep`] onward, suspending if a reaction window
/// queues mid-step.
///
/// Each loop iteration starts by checking for a queued reaction
/// window: if one is pending, the driver opens it and returns
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput). The window's
/// close path ([`close_reaction_window`]) re-enters this driver on
/// resume.
///
/// Fully frame-driven (Slice D #423): every effect a step would run is
/// `push_effect`'d for the global `drive` loop rather than run synchronously.
/// When a step pushes an effect (or a sub-step queues a reaction window), the
/// pushed frame becomes `last()`; the top-of-loop check below sees a
/// non-`SkillTest` top frame and returns `Done`, ceding to `drive`, which
/// resolves the sub-frame and re-dispatches this `SkillTest` at the
/// pre-advanced cursor. Each step pre-advances the cursor **before** pushing, so
/// a suspending effect never re-runs its step.
///
/// Step → next-continuation mapping (RR p.26 ST order):
///
/// - [`Resolving`](SkillTestStep::Resolving) → ST.3–ST.6 computation; advance to
///   [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions).
/// - [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions) → fire
///   `SuccessfullyInvestigated` (Investigate only) on the ST.6 success, before
///   the ST.7 consequences; advance to
///   [`FireOnCommit`](SkillTestStep::FireOnCommit).
/// - [`FireOnCommit`](SkillTestStep::FireOnCommit) → ST.7 head (`OnCommit`
///   effects); advance to [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp).
/// - [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp) → ST.7 action follow-up
///   (the clue discovery); advance to
///   [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect).
/// - [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect) → push the
///   success/failure card effect; advance to
///   [`FireOnResolution`](SkillTestStep::FireOnResolution).
/// - [`FireOnResolution`](SkillTestStep::FireOnResolution) → fire committed
///   cards' `OnSkillTestResolution` triggers (one per visit); advance to
///   [`PostRetaliate`](SkillTestStep::PostRetaliate).
/// - [`PostRetaliate`](SkillTestStep::PostRetaliate) → fire a Retaliate attack
///   (failed Fight vs ready retaliate enemy); advance to
///   [`PostOnResolution`](SkillTestStep::PostOnResolution).
/// - [`PostOnResolution`](SkillTestStep::PostOnResolution) → ST.8: discard
///   committed cards, emit [`SkillTestEnded`](crate::Event::SkillTestEnded),
///   drain pending modifiers, tear down the frame, return `Done`.
///
/// [`close_reaction_window`]: super::reaction_windows::close_reaction_window
pub(super) fn advance(cx: &mut Cx) -> EngineOutcome {
    loop {
        // A sub-frame queued by the previous step sits *above* this `SkillTest`
        // frame — a reaction window, or a pushed on_success/on_fail effect
        // (Slice D #423). Whenever the top frame is not this `SkillTest`, yield to
        // the `drive` loop: it drives the sub-frame, and its completion
        // re-dispatches this `SkillTest` at the pre-advanced cursor. A forced run
        // *below* this frame (#213 reentrancy: two Frozen in Fear copies) is never
        // `last()` while this driver runs, so "not the SkillTest on top" simply
        // means "a sub-frame above me" — no `rposition` self-location.
        if !matches!(
            cx.state.continuations.last(),
            Some(Continuation::SkillTest(_))
        ) {
            return EngineOutcome::Done;
        }

        let (continuation, investigator, indices_u8) = {
            let in_flight = cx.state.current_skill_test().unwrap_or_else(|| {
                unreachable!(
                    "advance: the SkillTest frame must exist while driver is active; \
                     state-corruption invariant violation"
                )
            });
            (
                in_flight.continuation,
                in_flight.investigator,
                in_flight.committed_by_active.clone(),
            )
        };

        match continuation {
            SkillTestStep::PreCommitWindow => {
                // RR p.26 player window after ST.1. (#374.)
                return open_skill_test_player_window(cx, SkillTestStep::AwaitingCommit, false);
            }
            SkillTestStep::AwaitingCommit => {
                // The frame's `awaiting()`: emit the commit prompt. (See
                // `emit_commit_window` for the propagation rationale.)
                return emit_commit_window(cx, investigator);
            }
            SkillTestStep::PreTokenWindow => {
                // RR p.26 player window after ST.2. (#374.)
                return open_skill_test_player_window(cx, SkillTestStep::Resolving, true);
            }
            SkillTestStep::Resolving => {
                // RR ST.3–ST.6 computation (sum icons, chaos token). Pushes
                // nothing; pre-advances the cursor to EmitSuccessReactions, so
                // the loop reads it next on this same frame.
                run_resolution(cx, investigator, &indices_u8);
            }
            SkillTestStep::EmitSuccessReactions {
                succeeded,
                failed_by,
            } => {
                // RR ST.6→ST.7 boundary. Pre-advances the cursor to FireOnCommit,
                // then fires the "after you successfully investigate" timing point
                // (Investigate + success only) — BEFORE the ST.7 consequences (the
                // clue discovery in ApplyFollowUp, the result effects in
                // ApplyResultEffect), so those can see any state the ST.6 reactions
                // changed. Yields if a 2+ forced run suspends.
                let outcome = emit_success_reactions_step(cx, investigator, succeeded, failed_by);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome;
                }
            }
            SkillTestStep::FireOnCommit {
                succeeded,
                failed_by,
            } => {
                // RR ST.7 head. Pre-advances the cursor to ApplyFollowUp, then
                // pushes the committed cards' OnCommit effects (Vicious Blow's
                // BoostAttackDamage) as one Seq. The loop drives them (non-
                // suspending stat boosts in scope) and re-dispatches at
                // ApplyFollowUp, which reads the now-populated accumulator.
                fire_on_commit_step(cx, investigator, &indices_u8, succeeded, failed_by);
            }
            SkillTestStep::ApplyFollowUp {
                succeeded,
                failed_by,
            } => {
                // RR ST.7 part 1. Pre-advances the cursor to ApplyResultEffect,
                // then applies the action follow-up (success-only). The
                // Investigate follow-up pushes discover_clue → the loop check at
                // the top of the next iteration sees the Effect frame on top and
                // yields; Fight/Evade/None mutate synchronously (a Fight that
                // defeats an enemy queues a reaction window — also a non-SkillTest
                // top frame, also yields).
                apply_follow_up_step(cx, investigator, succeeded, failed_by);
            }
            SkillTestStep::ApplyResultEffect {
                succeeded,
                failed_by,
            } => {
                apply_result_effect_step(cx, investigator, succeeded, failed_by);
            }
            SkillTestStep::FireOnResolution { succeeded, next } => {
                fire_on_resolution_step(cx, investigator, &indices_u8, succeeded, next);
            }
            SkillTestStep::PostRetaliate { succeeded } => {
                // Advance the cursor first: a retaliate that suspends on its
                // cancel/soak window resumes here at PostOnResolution (the retaliate
                // already happened; only its window is being resolved).
                cx.state
                    .current_skill_test_mut()
                    .expect("the SkillTest frame must persist across driver steps")
                    .continuation = SkillTestStep::PostOnResolution { succeeded };
                let outcome = fire_retaliate_if_any(cx, investigator, succeeded);
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    return outcome; // parked on the retaliate's window; resume via advance
                }
            }
            SkillTestStep::PostOnResolution { succeeded: _ } => {
                // RR ST.8 teardown. "After you successfully investigate"
                // (Obscuring Fog forced + Dr. Milan reaction) already fired at the
                // EmitSuccessReactions step via `emit_event`
                // (`SuccessfullyInvestigated`, #213) — forced-before-reaction.
                discard_committed_cards(cx, investigator, &indices_u8);
                cx.events.push(Event::SkillTestEnded { investigator });
                // ModifierScope::ThisSkillTest contributions expire when
                // the test ends. Drain pending entries for *this*
                // investigator only — entries queued for other
                // investigators' future tests stay.
                cx.state
                    .pending_skill_modifiers
                    .retain(|m| m.investigator != investigator);
                // Encounter-treachery disposal is no longer the skill-test
                // driver's concern (#380): a treachery whose Revelation
                // suspended into this test parks an `EncounterCard` frame
                // beneath it, which the framework disposes of at the
                // `resolve_input` chokepoint once this test completes.
                // Tear down the test's SkillTest frame (Axis-B T4), which also
                // carries the test data (#348). `take_skill_test` removes the
                // (unique — no nesting today) frame by position, so a player-
                // window gate legitimately sitting above it (#69/#70/#71) is
                // unaffected.
                let taken = cx.state.take_skill_test();
                debug_assert!(
                    taken.is_some(),
                    "skill-test teardown: no SkillTest frame on the continuation stack",
                );
                // Teardown tail. The test is fully torn down; cede to the `drive`
                // loop, which re-dispatches whatever it was nested within. A
                // suspending `EndOfTurn` forced stranded `end_turn` before
                // rotation and flagged the `InvestigatorTurn { ending: true }`
                // frame beneath (a single Frozen in Fear, or a 2+ forced run);
                // the loop's `InvestigatorTurn { ending: true }` arm runs
                // `resume_end_turn` once this returns — no reach-down here
                // (#434, unified with the former 2+ `EndOfTurnAfterForced` path).
                return EngineOutcome::Done;
            }
        }
    }
}

/// Validate that every entry in `indices` is a unique in-bounds hand
/// index for `investigator`, and return them downcast to `u8` (the
/// width hand indices use elsewhere in state).
fn validate_commit_indices(
    state: &GameState,
    investigator: InvestigatorId,
    indices: &[u32],
) -> Result<Vec<u8>, EngineOutcome> {
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "validate_commit_indices: investigator {investigator:?} disappeared while test \
             was in flight; this is a state-corruption invariant violation"
        )
    });
    // Arkham's upkeep hand-size limit caps hands well below 256 cards
    // in practice (#111 tracks the engine-side enforcement of the
    // discard-to-max-hand-size step), so the `u8::try_from` below
    // succeeds for every index that passed the bounds check. No
    // defensive overflow-rejection branch needed.
    let hand_len = inv.hand.len();
    let mut indices_u8: Vec<u8> = Vec::with_capacity(indices.len());
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for &i in indices {
        if !seen.insert(i) {
            return Err(EngineOutcome::Rejected {
                reason: format!("skill-test commit: duplicate hand index {i}").into(),
            });
        }
        if (i as usize) >= hand_len {
            return Err(EngineOutcome::Rejected {
                reason: format!(
                    "skill-test commit: hand index {i} out of bounds (hand size {hand_len})"
                )
                .into(),
            });
        }
        indices_u8.push(
            u8::try_from(i)
                .expect("bounds check above guarantees i < hand_len <= u8::MAX (see #111)"),
        );
    }

    // Enforce per-card "Max N committed per skill test" caps (#311). The
    // limit is printed metadata (`CardKind::Skill.commit_limit`); a card
    // with no cap, or no registry entry, is unconstrained. No-op without an
    // installed registry — engine-only tests that don't touch card data
    // never commit real cards, mirroring `sum_skill_value`.
    if let Some(reg) = card_registry::current() {
        let mut counts: BTreeMap<&CardCode, u8> = BTreeMap::new();
        for &i in &indices_u8 {
            *counts.entry(&inv.hand[usize::from(i)]).or_insert(0) += 1;
        }
        for (code, count) in counts {
            let cap = (reg.metadata_for)(code).and_then(|m| match m.kind {
                crate::card_data::CardKind::Skill { commit_limit, .. } => commit_limit,
                _ => None,
            });
            if let Some(limit) = cap {
                if count > limit {
                    return Err(EngineOutcome::Rejected {
                        reason: format!(
                            "skill-test commit: {code} allows at most {limit} committed per skill \
                             test, but {count} were committed"
                        )
                        .into(),
                    });
                }
            }
        }
    }

    Ok(indices_u8)
}

/// Sum the four skill-value contributions: investigator's printed
/// stat, constant modifiers from cards in play, queued
/// [`ModifierScope::ThisSkillTest`] pushes, and the committed cards'
/// matching + wild icons.
///
/// Cards / scopes not addressed by an installed registry contribute
/// 0 — the same silent-skip policy `constant_skill_modifier` uses.
///
/// [`ModifierScope::ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
fn sum_skill_value(
    state: &GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    committed_indices: &[u8],
) -> i8 {
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "sum_skill_value: investigator {investigator:?} disappeared while test was in \
             flight; this is a state-corruption invariant violation"
        )
    });
    let base = inv.skills.value(skill);
    let icon_mod = sum_committed_icons(&inv.hand, committed_indices, skill);
    let constant_mod = card_registry::current().map_or(0, |reg| {
        constant_skill_modifier(state, reg, investigator, skill, kind)
    });
    let pending_mod = pending_skill_modifier(state, investigator, skill);
    // One-shot modifier the initiating effect snapshotted (a weapon's
    // "+N for this attack"); 0 for player-action tests.
    let test_mod = state.current_skill_test().map_or(0, |t| t.test_modifier);
    base.saturating_add(constant_mod)
        .saturating_add(pending_mod)
        .saturating_add(icon_mod)
        .saturating_add(test_mod)
}

/// Sum the skill-icon contribution from the cards at `indices` in
/// `hand`: each card adds its matching-skill icons plus its wild
/// icons. Cards whose code isn't in the installed registry contribute
/// 0; no registry installed = 0 contribution overall.
fn sum_committed_icons(hand: &[CardCode], indices: &[u8], skill: SkillKind) -> i8 {
    let Some(reg) = card_registry::current() else {
        return 0;
    };
    indices
        .iter()
        .map(|&idx| {
            let code = &hand[usize::from(idx)];
            (reg.metadata_for)(code).map_or(0_i8, |meta| {
                let icons = meta.skill_icons();
                let matching = match skill {
                    SkillKind::Willpower => icons.willpower,
                    SkillKind::Intellect => icons.intellect,
                    SkillKind::Combat => icons.combat,
                    SkillKind::Agility => icons.agility,
                };
                let raw = matching.saturating_add(icons.wild);
                i8::try_from(raw).unwrap_or(i8::MAX)
            })
        })
        .fold(0_i8, i8::saturating_add)
}

/// Advance the RNG, draw a chaos token, compute the clamped total
/// against `difficulty`, and emit the per-step events
/// (`ChaosTokenRevealed` + either `SkillTestSucceeded` or
/// `SkillTestFailed`). Returns `true` on success so the caller can
/// branch its follow-up.
///
/// All arithmetic stays in `i8` with saturating ops: realistic
/// gameplay values (skill 1–8, modifier ±8, difficulty ≤ ~6) fit far
/// inside `i8`, but saturation defends against absurd state
/// configurations without needing a wider integer type.
fn resolve_chaos_token_and_emit(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    skill_value: i8,
) -> (bool, u8) {
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];

    // Symbol tokens may route to the active scenario's reference-card
    // effects (modifier + deferred side effects). Numeric/AutoFail/
    // ElderSign never do; nor do scenarios without a hook (static path).
    let symbol_outcome = match token {
        ChaosToken::Skull | ChaosToken::Cultist | ChaosToken::Tablet | ChaosToken::ElderThing => {
            crate::scenario::resolve_symbol_token(cx.state, token, investigator)
        }
        _ => None,
    };

    let resolution = match &symbol_outcome {
        Some(outcome) => TokenResolution::Modifier(outcome.modifier),
        None => resolve_token(token, &cx.state.token_modifiers),
    };
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && fail_reason.is_none();
    let failed_by = if succeeded {
        0
    } else {
        difficulty.saturating_sub(total)
    };
    if succeeded {
        cx.events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        cx.events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason,
            by: failed_by,
        });
    }

    // Symbol side effects resolve after success/failure is known.
    if let Some(outcome) = symbol_outcome {
        apply_symbol_outcome(cx, investigator, &outcome, succeeded);
    }

    (succeeded, u8::try_from(failed_by).unwrap_or(0))
}

/// Move every committed hand card to the controller's discard pile,
/// emitting [`Event::CardDiscarded`] for each. Per the
/// [`Event::SkillTestEnded`] docs, these discards precede the
/// `SkillTestEnded` cleanup marker. Walk indices in descending order
/// so each `remove` keeps the still-pending indices stable.
fn discard_committed_cards(cx: &mut Cx, investigator: InvestigatorId, indices_u8: &[u8]) {
    let mut sorted: Vec<u8> = indices_u8.to_vec();
    sorted.sort_by(|a, b| b.cmp(a));
    // Collect discarded codes first (releasing the mutable borrow on
    // cx.state) so we can push to cx.events afterwards without a
    // simultaneous borrow through cx.
    let discarded: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get_mut(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "discard_committed_cards: investigator {investigator:?} vanished after \
                     follow-up; this is a state-corruption invariant violation"
                )
            });
        sorted
            .iter()
            .map(|&idx| {
                let code = inv.hand.remove(usize::from(idx));
                inv.discard.push(code.clone());
                code
            })
            .collect()
    };
    for code in discarded {
        cx.events.push(Event::CardDiscarded {
            investigator,
            code,
            from: Zone::Hand,
        });
    }
}

/// Dispatch the action-specific on-success follow-up for the resolving
/// skill test (RR ST.7). Runs only on success (the caller gates on it).
///
/// The Investigate follow-up *pushes* its `discover_clue` effect for the global
/// drive loop (Slice D #423) — `advance` then yields, the loop drives the
/// discovery (suspending on Cover Up 01007's before-interrupt if needed), and on
/// completion re-dispatches the `SkillTest` at
/// [`ApplyResultEffect`](SkillTestStep::ApplyResultEffect). The "after you
/// successfully investigate" timing point already fired at the preceding
/// [`EmitSuccessReactions`](SkillTestStep::EmitSuccessReactions) step — on the
/// ST.6 success, before this ST.7 discovery. Fight / Evade / None mutate
/// synchronously and push nothing.
fn apply_skill_test_follow_up(
    cx: &mut Cx,
    investigator: InvestigatorId,
    follow_up: SkillTestFollowUp,
) {
    match follow_up {
        SkillTestFollowUp::None => {}
        SkillTestFollowUp::Investigate => {
            // Push discover_clue for the drive loop. It may suspend on a
            // before-timing interrupt (Cover Up 01007); the loop drives it to
            // completion either way, then re-dispatches this SkillTest at
            // `ApplyResultEffect`. The "after you successfully investigate"
            // timing point already fired at the preceding EmitSuccessReactions
            // step, before this discovery. The Investigate follow-up has no
            // source card, so `for_controller` is correct.
            let effect = discover_clue(LocationTarget::YourLocation, 1);
            push_effect(cx, &effect, EvalContext::for_controller(investigator));
        }
        SkillTestFollowUp::Fight {
            enemy,
            extra_damage,
        } => {
            // Mid-test enemy disappearance isn't possible in Phase 3
            // (no commit-window effects mutate enemies), so
            // damage_enemy's enemy-missing panic stays loud. A weapon's
            // bonus damage (.38 Special's +1) rides on `extra_damage`; a
            // committed skill's bonus (Vicious Blow's +1) accumulates on
            // the in-flight record at commit time (#307). The in-flight
            // test is still present here — it's torn down only at the end
            // of resolution — so the accumulator is readable.
            let bonus = cx
                .state
                .current_skill_test()
                .map_or(0, |t| t.bonus_attack_damage);
            super::combat::damage_enemy(
                cx,
                enemy,
                1u8.saturating_add(extra_damage).saturating_add(bonus),
                Some(investigator),
            );
        }
        SkillTestFollowUp::Evade { enemy } => {
            let e = cx.state.enemies.get_mut(&enemy).unwrap_or_else(|| {
                unreachable!(
                    "Evade follow-up: enemy {enemy:?} vanished while test was in flight; \
                     this is a state-corruption invariant violation"
                )
            });
            e.engaged_with = None;
            e.exhausted = true;
            cx.events.push(Event::EnemyDisengaged {
                enemy,
                investigator,
            });
            cx.events.push(Event::EnemyExhausted { enemy });
        }
    }
}

/// Fire a Retaliate attack if the just-resolved test was a *failed Fight*
/// against a ready enemy with the retaliate keyword.
///
/// Rules Reference p.18: *"Each time an investigator fails a skill test
/// while attacking a ready enemy with the retaliate keyword, after
/// applying all results for that skill test, that enemy performs an
/// attack against the attacking investigator. An enemy does not exhaust
/// after performing a retaliate attack."*
///
/// Runs at the `PostRetaliate` step — after `fire_on_skill_test_resolution`
/// (the rest of ST.7) and before the `PostOnResolution` teardown (ST.8) —
/// matching "after applying all results." Routes through
/// [`super::combat::drive_retaliate`] so the attack opens its before-attack
/// cancel window (Dodge 01023) and per-soaked-asset soak window (Guard Dog
/// 01021) (#379). Returns [`AwaitingInput`] if a window suspends, [`Done`]
/// otherwise. Non-exhausting (RR p.18) — honored by
/// [`EnemyAttackSource::Retaliate`] inside `drive_retaliate`.
///
/// No-op unless every condition holds: the test failed; its follow-up was
/// `Fight`; the enemy is still in play, ready (`!exhausted`), and has
/// `retaliate`. A missing enemy is skipped quietly — a failed fight deals
/// no damage, so the target can't have been defeated mid-test; this only
/// guards against future enemy-removing commit effects.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
/// [`Done`]: crate::engine::EngineOutcome::Done
/// [`EnemyAttackSource::Retaliate`]: crate::state::EnemyAttackSource::Retaliate
fn fire_retaliate_if_any(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
) -> EngineOutcome {
    if succeeded {
        return EngineOutcome::Done;
    }
    let follow_up = cx.state.current_skill_test().map(|t| t.follow_up);
    let Some(SkillTestFollowUp::Fight { enemy, .. }) = follow_up else {
        return EngineOutcome::Done;
    };
    let retaliates = cx
        .state
        .enemies
        .get(&enemy)
        .is_some_and(|e| e.retaliate && !e.exhausted);
    if retaliates {
        // Route through the attack loop (#379) so the retaliate opens its cancel
        // (Dodge) and soak (Guard Dog) windows; non-exhausting (RR p.18).
        super::combat::drive_retaliate(cx, enemy, investigator)
    } else {
        EngineOutcome::Done
    }
}

/// Collect, in committed-card order, the effects of every committed card's
/// [`Trigger::OnSkillTestResolution`] ability that matches the resolved
/// outcome.
///
/// The [`FireOnResolution`](SkillTestStep::FireOnResolution) driver step
/// indexes into this flat list, pushing one effect per visit so they
/// cursor-sequence (no LIFO). At collection time the committed cards are still
/// in hand at their hand indices (discard happens at teardown) and the
/// in-flight record still holds the tested location, so
/// [`LocationTarget::TestedLocation`] resolves cleanly.
///
/// No registry installed → empty list: engine-only tests that don't touch card
/// data never reach `OnSkillTestResolution`. Silent skip mirrors
/// `constant_skill_modifier`'s behavior.
fn collect_on_skill_test_resolution(
    cx: &Cx,
    investigator: InvestigatorId,
    indices_u8: &[u8],
    succeeded: bool,
) -> Vec<card_dsl::dsl::Effect> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let outcome_now = if succeeded {
        crate::dsl::TestOutcome::Success
    } else {
        crate::dsl::TestOutcome::Failure
    };

    // Each committed index resolves to a hand-position CardCode; the cards are
    // still in hand at this point (discard happens at teardown).
    let codes: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "collect_on_skill_test_resolution: investigator {investigator:?} vanished \
                 while test was in flight; this is a state-corruption invariant violation"
                )
            });
        indices_u8
            .iter()
            .map(|&i| inv.hand[usize::from(i)].clone())
            .collect()
    };

    let mut effects = Vec::new();
    for code in &codes {
        let Some(abilities) = (reg.abilities_for)(code) else {
            continue;
        };
        for ability in abilities {
            let Trigger::OnSkillTestResolution { outcome } = ability.trigger else {
                continue;
            };
            if outcome != outcome_now {
                continue;
            }
            effects.push(ability.effect);
        }
    }
    effects
}

/// Collect, in committed-card order, the effects of every committed card's
/// [`Trigger::OnCommit`] ability.
///
/// The [`FireOnCommit`](SkillTestStep::FireOnCommit) driver step combines these
/// into one [`Effect::Seq`](crate::dsl::Effect::Seq) and `push_effect`s it for
/// the drive loop. They run at the head of ST.7 (after the token resolves) but
/// before [`ApplyFollowUp`](SkillTestStep::ApplyFollowUp) reads the
/// `bonus_attack_damage` accumulator the in-scope consumer
/// ([`Effect::BoostAttackDamage`](crate::dsl::Effect::BoostAttackDamage),
/// Vicious Blow 01025) populates — and that consumer is conditional on success
/// ("If this skill test is successful during an attack…"). The committed cards
/// are still in hand at this point (discard happens at teardown).
///
/// No registry installed → empty list: engine-only tests that don't touch card
/// data never commit real cards. Silent skip mirrors
/// `constant_skill_modifier`'s behavior.
fn collect_on_commit(
    cx: &Cx,
    investigator: InvestigatorId,
    indices_u8: &[u8],
) -> Vec<card_dsl::dsl::Effect> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    // Snapshot the committed hand codes. The cards are still in hand at commit.
    let codes: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "collect_on_commit: investigator {investigator:?} vanished while test was in \
                     flight; this is a state-corruption invariant violation"
                )
            });
        indices_u8
            .iter()
            .map(|&i| inv.hand[usize::from(i)].clone())
            .collect()
    };

    let mut effects = Vec::new();
    for code in &codes {
        let Some(abilities) = (reg.abilities_for)(code) else {
            continue;
        };
        for ability in abilities {
            if !matches!(ability.trigger, Trigger::OnCommit) {
                continue;
            }
            effects.push(ability.effect);
        }
    }
    effects
}

/// Public dispatch wrapper for [`PlayerAction::PerformSkillTest`].
///
/// Opens the commit window with no action-specific follow-up. The
/// after-resolution trigger window (#64) is downstream.
pub(super) fn perform_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    start_skill_test(
        cx,
        investigator,
        skill,
        SkillTestKind::Plain,
        difficulty,
        SkillTestFollowUp::None,
        None,
        None,
        None,
        0, // bare PerformSkillTest: no effect modifier
    )
}

pub(super) fn peril_check(
    _cx: &mut Cx,
    _code: &CardCode,
    _investigator: InvestigatorId,
    _is_peril: bool,
) {
    // TODO(future-peril-PR): if `is_peril`, install a temporary
    //   restriction on `_cx.state` such that other investigators cannot
    //   (a) play cards, (b) trigger abilities, or (c) commit to the
    //   drawing investigator's skill tests until this card's
    //   resolution completes.
}

/// Apply a resolved symbol token's side effects to the testing
/// investigator: `immediate` effects always, `on_fail` effects only when
/// the test failed. Routes through the same elimination paths as
/// `Effect::Deal`, so defeat handling and the `DamageTaken` /
/// `HorrorTaken` events are reused.
fn apply_symbol_outcome(
    cx: &mut Cx,
    investigator: InvestigatorId,
    outcome: &crate::scenario::SymbolOutcome,
    succeeded: bool,
) {
    use crate::scenario::TokenEffect;
    let mut effects: Vec<TokenEffect> = outcome.immediate.clone();
    if !succeeded {
        effects.extend(outcome.on_fail.iter().copied());
    }
    for effect in effects {
        match effect {
            TokenEffect::Damage(n) => {
                crate::engine::dispatch::elimination::take_damage(cx, investigator, n);
            }
            TokenEffect::Horror(n) => {
                crate::engine::dispatch::elimination::take_horror(cx, investigator, n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_event;
    use crate::assert_no_event;
    use crate::event::Event;
    use crate::scenario::{SymbolOutcome, TokenEffect};
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// The `Fight` follow-up deals `1 + extra_damage + bonus_attack_damage`,
    /// reading the commit-time accumulator off the in-flight record
    /// (Vicious Blow 01025). With `extra_damage: 1` (a weapon bonus) and
    /// `bonus_attack_damage: 2`, the attack deals `1 + 1 + 2 = 4`.
    #[test]
    fn fight_follow_up_adds_bonus_attack_damage() {
        use crate::state::EnemyId;
        use crate::test_support::test_enemy;

        let inv = InvestigatorId(1);
        let mut enemy = test_enemy(7, "Goon");
        enemy.max_health = 10; // avoid clamping so the dealt damage is observable
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(InFlightSkillTest {
                investigator: inv,
                skill: SkillKind::Combat,
                kind: SkillTestKind::Fight,
                difficulty: 2,
                committed_by_active: Vec::new(),
                tested_location: None,
                follow_up: SkillTestFollowUp::Fight {
                    enemy: EnemyId(7),
                    extra_damage: 1,
                },
                on_fail: None,
                on_success: None,
                source: None,
                continuation: SkillTestStep::AwaitingCommit,
                test_modifier: 0,
                bonus_attack_damage: 2,
            }));
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        apply_skill_test_follow_up(
            &mut cx,
            inv,
            SkillTestFollowUp::Fight {
                enemy: EnemyId(7),
                extra_damage: 1,
            },
        );

        assert_eq!(
            state.enemies[&EnemyId(7)].damage,
            4,
            "1 base + 1 extra_damage + 2 bonus_attack_damage"
        );
    }

    #[test]
    fn apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure() {
        let inv = InvestigatorId(1);

        let run = |succeeded: bool, outcome: SymbolOutcome| {
            let mut state = GameStateBuilder::new()
                .with_investigator(test_investigator(1))
                .build();
            let mut events = Vec::new();
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            apply_symbol_outcome(&mut cx, inv, &outcome, succeeded);
            events
        };

        // Case 1: immediate Damage(1), test succeeded → DamageTaken present.
        let ev = run(
            true,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![TokenEffect::Damage(1)],
                on_fail: vec![],
            },
        );
        assert_event!(ev, Event::DamageTaken { investigator, amount: 1 } if *investigator == inv);

        // Case 2: on_fail Horror(1), test succeeded → HorrorTaken absent.
        let ev = run(
            true,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![],
                on_fail: vec![TokenEffect::Horror(1)],
            },
        );
        assert_no_event!(ev, Event::HorrorTaken { .. });

        // Case 3: on_fail Horror(1), test failed → HorrorTaken present.
        let ev = run(
            false,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![],
                on_fail: vec![TokenEffect::Horror(1)],
            },
        );
        assert_event!(ev, Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv);
    }

    // The former `revelation_skill_test_failure_deals_margin_damage_and_discards`
    // unit test is gone (#380): it simulated the removed
    // `pending_revelation_discard` slot and drove `finish_skill_test` directly,
    // bypassing the new `resolve_input`-chokepoint disposal. The real
    // suspended-Revelation-into-skill-test discard is integration-tested by
    // `crates/cards/tests/revelation_treacheries.rs::grasping_hands_*`
    // (01162), and the margin-damage math by the same test.

    /// A plain (non-revelation) skill test disposes of no encounter card — the
    /// skill-test driver no longer touches encounter disposal at all (#380).
    #[test]
    fn plain_skill_test_disposes_of_no_encounter_card() {
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = perform_skill_test(&mut cx, inv, SkillKind::Intellect, 1);
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert!(state.encounter_discard.is_empty());
    }

    /// A skill test with an `on_success` effect runs it on a successful
    /// draw (the success-side mirror of the `on_fail` path).
    #[test]
    fn skill_test_runs_on_success_effect_on_a_passing_draw() {
        use crate::dsl::{deal_horror, InvestigatorTarget};
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        // Willpower 3 + Numeric(0) = 3 vs difficulty 2 → success.
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            Some(deal_horror(InvestigatorTarget::You, 1)),
            None,
            None,
            0,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&inv].horror, 1,
            "on_success effect ran on the passing draw",
        );
    }

    /// Both ST.1/ST.2 player windows open and auto-skip (no registry / nothing
    /// Fast-eligible), bracketing the commit, and the test still resolves. (#374.)
    #[test]
    fn skill_test_opens_and_auto_skips_both_player_windows() {
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // start -> PreCommitWindow auto-skips window 1 -> parks at AwaitingCommit.
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            None,
            None,
            None,
            0,
        );
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit prompt (window 1 before commit opened and auto-skipped to it)"
        );

        // commit nothing -> PreTokenWindow auto-skips window 2 -> resolves to end.
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(
            out,
            EngineOutcome::Done,
            "window 2 (before token) opened and auto-skipped, then resolved",
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the test resolved to the end: {events:?}",
        );
    }

    /// Closing a skill-test player window re-enters `advance` at the
    /// pre-advanced cursor (the `run_fast_continuation` arm), not just via the
    /// auto-skip path. Here window 1 is "about to close" — the cursor is already
    /// `AwaitingCommit` — so `run_fast_continuation` must re-enter `advance` and
    /// emit the commit prompt. (#374.)
    #[test]
    fn closing_a_skill_test_player_window_re_enters_advance() {
        use crate::state::{ChaosToken, Continuation, FastWindowKind, InFlightSkillTest};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        // A SkillTest pre-advanced to AwaitingCommit, as if window 1 just opened.
        state
            .continuations
            .push(Continuation::SkillTest(InFlightSkillTest {
                investigator: inv,
                skill: SkillKind::Willpower,
                kind: SkillTestKind::Plain,
                difficulty: 2,
                committed_by_active: Vec::new(),
                tested_location: None,
                follow_up: SkillTestFollowUp::None,
                on_fail: None,
                on_success: None,
                source: None,
                continuation: SkillTestStep::AwaitingCommit,
                test_modifier: 0,
                bonus_attack_damage: 0,
            }));
        let mut events = Vec::new();
        let out = super::super::reaction_windows::run_fast_continuation(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            FastWindowKind::SkillTest {
                before_token: false,
            },
        );
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the commit prompt after the window closed, got {out:?}");
        };
        assert!(
            request.prompt.contains("Commit cards"),
            "re-entered advance at AwaitingCommit: {request:?}",
        );
    }

    /// The reified driver: `start_skill_test` parks at `AwaitingCommit` via
    /// `advance` (emitting the commit prompt), and committing drives the test to
    /// teardown — `SkillTestStarted` then `SkillTestEnded`, no frame left behind.
    #[test]
    fn commit_emits_then_resolves_through_advance() {
        use crate::state::{ChaosToken, Continuation};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            None,
            None,
            None,
            0,
        );
        // advance parked at AwaitingCommit and emitted the commit prompt.
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the commit prompt, got {out:?}");
        };
        assert!(
            request.prompt.contains("Commit cards"),
            "the AwaitingCommit arm emits the commit prompt: {request:?}",
        );
        assert!(matches!(
            cx.state.continuations.last(),
            Some(Continuation::SkillTest(_))
        ));

        // Commit nothing → the hop parks; the loop drives to teardown.
        let out = finish_skill_test(&mut cx, &[]);
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SkillTestStarted { .. }))
                && events
                    .iter()
                    .any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the test ran start-to-end: {events:?}",
        );
        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::SkillTest(_))),
            "the SkillTest frame was torn down",
        );
    }

    /// The commit hop parks the resolution for the loop rather than driving it
    /// itself: `finish_skill_test` returns `Done` with the `SkillTest` frame on
    /// top at `PreTokenWindow` and emits no `SkillTestEnded`; the `drive` loop's
    /// `SkillTest` arm then resolves it to teardown. (Slice C, #431 — commit-hop
    /// re-entry retired.)
    #[test]
    fn finish_skill_test_parks_the_resolution_for_the_loop() {
        use crate::state::{ChaosToken, Continuation, SkillTestStep};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // Park at AwaitingCommit (the commit prompt).
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            None,
            None,
            None,
            0,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));

        // Commit nothing: the hop PARKS — it must not itself resolve the test.
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            matches!(
                cx.state.continuations.last(),
                Some(Continuation::SkillTest(t)) if matches!(t.continuation, SkillTestStep::PreTokenWindow)
            ),
            "the commit hop parks the SkillTest at PreTokenWindow for the loop to drive",
        );
        assert!(
            !cx.events
                .iter()
                .any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the hop itself does not resolve the test to teardown",
        );

        // The loop's SkillTest arm drives the parked frame the rest of the way.
        let out = super::super::drive(&mut cx, out);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            cx.events
                .iter()
                .any(|e| matches!(e, Event::SkillTestEnded { .. })),
            "the loop resolved the test to teardown",
        );
        assert!(
            !cx.state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::SkillTest(_))),
            "the SkillTest frame was torn down by the loop",
        );
    }

    fn substitution_state(inv: InvestigatorId) -> GameState {
        use crate::state::{ChaosToken, SkillSubstitution};
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        state.skill_substitutions.push(SkillSubstitution {
            investigator: inv,
            use_skill: SkillKind::Intellect,
            for_skills: vec![SkillKind::Combat, SkillKind::Agility],
        });
        state
    }

    #[test]
    fn combat_test_with_substitution_prompts_then_becomes_intellect_on_yes() {
        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            // test_modifier 2 stands in for a weapon's +combat bonus.
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                2,
            )
        };
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "prompt");
        assert!(
            matches!(state.continuations.last(), Some(crate::state::Continuation::SubstitutionPrompt { investigator }) if *investigator == inv)
        );

        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
        };
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            super::super::drive(&mut cx, out)
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit window"
        );
        let t = state.current_skill_test().unwrap();
        assert_eq!(t.skill, SkillKind::Intellect, "now an intellect test");
        assert_eq!(t.kind, SkillTestKind::Fight, "still a Fight (damage)");
        assert_eq!(t.test_modifier, 0, "weapon combat bonus dropped");
        assert!(!matches!(
            state.continuations.last(),
            Some(crate::state::Continuation::SubstitutionPrompt { .. })
        ));
    }

    #[test]
    fn substitution_prompt_keeps_the_test_on_its_frame() {
        // The substitution prompt suspends *before* the commit window — the one
        // place test data exists pre-commit. Pin that it lives on a SkillTest
        // frame (not a removed Option field) during that window (#348).
        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                0,
            )
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "substitution prompt should suspend",
        );
        assert!(
            matches!(state.continuations.last(), Some(crate::state::Continuation::SubstitutionPrompt { investigator }) if *investigator == inv)
        );
        assert!(
            state.current_skill_test().is_some(),
            "the in-flight test must live on a SkillTest frame during the \
             substitution prompt, not in a removed Option field",
        );
        assert!(
            state
                .continuations
                .iter()
                .any(|c| matches!(c, crate::state::Continuation::SkillTest(_))),
            "a SkillTest frame is on the stack before the commit window",
        );
    }

    #[test]
    fn substitution_choice_no_keeps_the_printed_skill() {
        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let _ = start_skill_test(
                &mut cx,
                inv,
                SkillKind::Agility,
                SkillTestKind::Evade,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                0,
            );
        }
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(1)))
        };
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            super::super::drive(&mut cx, out)
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit window"
        );
        assert_eq!(
            state.current_skill_test().unwrap().skill,
            SkillKind::Agility,
            "declined — keeps the printed skill",
        );
    }

    /// The substitution resume parks the test for the loop rather than driving to
    /// the commit window itself: choosing the substitution pops the
    /// `SubstitutionPrompt`, rewrites the skill, and returns `Done` with the
    /// `SkillTest` on top; the `drive` loop then opens the commit window. (Slice C,
    /// #431 — substitution-resume re-entry retired.)
    #[test]
    fn resume_substitution_choice_parks_for_the_loop() {
        use crate::state::Continuation;

        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                2,
            )
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "substitution prompt"
        );

        // Choose the substitution: the resume PARKS — it does not itself open the
        // commit window.
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
        };
        assert_eq!(
            out,
            EngineOutcome::Done,
            "the substitution resume parks for the loop"
        );
        assert!(
            !matches!(
                state.continuations.last(),
                Some(Continuation::SubstitutionPrompt { .. })
            ),
            "the SubstitutionPrompt was consumed",
        );
        assert!(
            matches!(state.continuations.last(), Some(Continuation::SkillTest(_))),
            "the SkillTest frame is parked on top for the loop to drive",
        );
        assert_eq!(
            state.current_skill_test().unwrap().skill,
            SkillKind::Intellect,
            "the substitution rewrote the skill before parking",
        );

        // The loop drives the parked test to its commit window.
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            super::super::drive(&mut cx, out)
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit window"
        );
    }

    #[test]
    fn no_active_substitution_opens_commit_window_directly() {
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![crate::state::ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                0,
            )
        };
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        assert!(
            !matches!(
                state.continuations.last(),
                Some(crate::state::Continuation::SubstitutionPrompt { .. })
            ),
            "no prompt"
        );
        assert_eq!(state.current_skill_test().unwrap().skill, SkillKind::Combat,);
    }
}
