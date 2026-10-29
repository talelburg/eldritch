//! The timing-sequence coordinator frames: the `when → at → after` cells of
//! one triggering condition, with the condition's own resolution between the
//! first two.
//!
//! [`super::emit::queue_event`] pushes a [`Continuation::EmitEvent`] for **every**
//! triggering condition, with no exceptions; the `drive` loop dispatches it
//! here.
//!
//! - [`dispatch_emit_event`] walks `When → ResolveCondition → At → After` —
//!   the three RR timing cells with the triggering condition's own resolution
//!   between the first two — pushing a
//!   [`Continuation::TimingPoint`] for each *populated* bucket and **re-scanning
//!   each cell fresh** (the per-cell eligibility re-scan — a `when` reaction can
//!   change whether an `at` forced fires; the grid is not pre-computed).
//! - [`dispatch_timing_point`] resolves one cell's forced-then-reaction
//!   (`sub` cursor `Forced → Reaction → Done`) — RR p.2's forced-before-reaction,
//!   for which this cursor is the only mechanism.
//! - A `when`-cell ability that **prevents** the condition suppresses the rest
//!   of the sequence — the resolve step, the `at` and `after` cells, and the
//!   remainder of the `when` cell itself (citations on
//!   [`EmitEventFrame::prevented`]).
//! - A condition an effect raised **parks** on that effect when it reaches its
//!   `at` cell, and its `at` and `after` cells open once the effect has
//!   resolved in full ([`park_in_effect_base`], ADR 0021).
//!
//! Neither driver suspends *itself*: each does one step and returns `Done`, and
//! the loop re-dispatches the (mutated) top frame — or `AwaitingInput` when the
//! step opens a window / forced run. On a child's pop the parent's cursor has
//! already been advanced, so re-dispatch makes progress (never re-scans the same
//! cell into a loop).

use card_dsl::dsl::EventTiming;

use crate::engine::dispatch::emit::ConditionResolution;
use crate::engine::dispatch::{forced_triggers, reaction_windows, trigger_scan};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{
    Continuation, EffectBaseFrame, EmitEventFrame, EmitStep, TimingPointFrame, TimingSub,
};

/// Dispatch the [`Continuation::EmitEvent`] coordinator on top of the stack
/// (called only by the `drive` loop with one on top). One step of the sequence
/// `When → ResolveCondition → At → After`:
///
/// - **a cell** (`When` / `At` / `After`) — re-scan it; if it holds any forced
///   or reaction ability, push a [`Continuation::TimingPoint`] and yield,
///   otherwise advance the cursor (popping the coordinator after `After`).
/// - **`ResolveCondition`** — step 2 of `glossary/Nested_Sequences.md`: the
///   triggering condition's own impact, run by the coordinator when the
///   condition is [`ConditionResolution::Coordinator`] and already done by the
///   emitting caller when it is [`ConditionResolution::Caller`]. The cursor
///   advances to `At` *before* the step runs, so a resolution that suspends
///   resumes at the `at` cell rather than resolving the condition twice.
///
/// Arriving at `At` inside an effect, the coordinator first parks on that
/// effect ([`park_in_effect_base`]) and leaves the stack, to come back at the
/// same cell once the effect has resolved.
///
/// A caller-owned condition's `when` cell is **not walked** — the caller has
/// already mutated, so an interrupt there would resolve after the thing it meant
/// to interrupt. An ability that declares one is rejected rather than dropped
/// (this project never silently no-ops); see [`ConditionResolution::Caller`] and
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
///
/// Visible to `engine` rather than to `dispatch` alone because the evaluator's
/// bounded test driver has to walk the coordinator too: an effect
/// leaf can emit a condition whose own resolution happens here, so a driver that
/// stopped at this frame would leave the effect half-resolved.
pub(in crate::engine) fn dispatch_emit_event(cx: &mut Cx) -> EngineOutcome {
    let EmitEventFrame {
        event,
        step,
        prevented,
    } = cx
        .state
        .continuations
        .top_expect::<EmitEventFrame>()
        .clone();
    if step == EmitStep::At && park_in_effect_base(cx) {
        return EngineOutcome::Done;
    }
    let resolution = event.condition_resolution();
    if step == EmitStep::ResolveCondition {
        return match resolution {
            ConditionResolution::Coordinator(resolve) => {
                if prevented {
                    // A `when`-cell ability prevented the condition, so neither
                    // step 2 nor the cells after it happen: the sequence is
                    // abandoned where it stands (see
                    // [`EmitEventFrame::prevented`] for the citations).
                    abandon_emit(cx);
                    EngineOutcome::Done
                } else {
                    // Advance *before* resolving, so a resolution that suspends
                    // resumes at the `at` cell rather than resolving twice.
                    advance_or_finish_emit(cx);
                    resolve(cx, &event)
                }
            }
            // Already resolved, by the caller, before it emitted — so there is
            // nothing here to prevent (a caller-owned condition's `when` cell is
            // not walked, so no ability of its own can have flagged it).
            ConditionResolution::Caller => {
                advance_or_finish_emit(cx);
                EngineOutcome::Done
            }
        };
    }
    let Some(bucket) = step.cell() else {
        unreachable!("dispatch_emit_event: ResolveCondition handled above");
    };
    let caller_owned = matches!(resolution, ConditionResolution::Caller);
    // Per-cell re-scan: the prior cell may have changed board state.
    let asked = reaction_windows::condition_as_it_now_stands(cx.state, cx.cards, &event, bucket);
    let forced = trigger_scan::collect_forced(cx.state, cx.cards, &asked, bucket);
    let has_forced = !forced.is_empty();
    let has_reaction =
        !trigger_scan::collect_reactions(cx.state, cx.cards, &asked, bucket).is_empty();
    if caller_owned && step == EmitStep::When {
        if has_forced || has_reaction {
            return EngineOutcome::Rejected {
                reason: format!(
                    "timing coordinator: {event:?} is a caller-owned triggering condition, so its \
                     `when` cell is not walked — an ability declaring interrupt timing on it \
                     cannot resolve before the condition, which has already resolved. Migrate the \
                     condition to a coordinator-owned arm: the caller-owned arm is \
                     scaffolding, migrated one condition at a time (worked examples: \
                     #703's clue discovery, #704's enemy attack — see \
                     docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md)."
                )
                .into(),
            };
        }
        advance_or_finish_emit(cx);
        return EngineOutcome::Done;
    }
    if has_forced || has_reaction {
        cx.state.continuations.push(TimingPointFrame {
            event,
            bucket,
            sub: TimingSub::Forced(forced),
        });
    } else {
        advance_or_finish_emit(cx);
    }
    EngineOutcome::Done
}

/// Dispatch the [`Continuation::TimingPoint`] on top of the stack (called only
/// by the `drive` loop). Runs the `sub` cursor `Forced → Reaction → Done`:
///
/// - **`Forced`** — fire the forced candidates the coordinator's scan cached on
///   the cursor (0/1 inline; 2+ via the ordered run). The cursor advances to
///   `Reaction` *before* firing, so a suspending 2+ run resumes at `Reaction`
///   rather than firing forced again.
/// - **`Reaction`** — open the bucket's reaction window if any candidate; else
///   finish the bucket. The cursor advances to `Done` before opening, so the
///   re-dispatch after the window closes finishes (never re-opens).
/// - **`Done`** — advance the parent `EmitEvent`'s cursor and pop self.
///
/// Visible to `engine` for the same reason as [`dispatch_emit_event`].
pub(in crate::engine) fn dispatch_timing_point(cx: &mut Cx) -> EngineOutcome {
    let TimingPointFrame { event, bucket, sub } = cx
        .state
        .continuations
        .top_expect::<TimingPointFrame>()
        .clone();
    if bucket == EventTiming::When && condition_is_prevented(cx) {
        // A `when`-cell ability just prevented the condition. The rest of *this*
        // cell is suppressed along with the cells after it — Dodge
        // 01023's ruling reaches the `when` cell itself. Any candidate still
        // pending in an open window of this cell was withdrawn by
        // `reaction_windows::withdraw_suppressed_candidates` before that window
        // closed; this is the cursor half, skipping the sub-steps not yet
        // started. Finishing the point hands back to the coordinator, which
        // reads the same flag at its resolve step and abandons the sequence.
        //
        // No ownership check is needed: a caller-owned condition's `when` cell is
        // never walked (the arm above returns before any `TimingPoint` is
        // pushed), so a `When` bucket here is coordinator-owned by construction.
        // The flag is this point's own parent's, so a condition emitted
        // while another is cancelled walks its own `when` cell untouched.
        finish_timing_point(cx);
        return EngineOutcome::Done;
    }
    // Both sub-steps work from the condition as it now stands: the `Forced`
    // step binds its candidates from it, and a forced `when` ability resolved
    // in that step can have flipped a skill test's verdict before the
    // `Reaction` step scans.
    let event = reaction_windows::condition_as_it_now_stands(cx.state, cx.cards, &event, bucket);
    match sub {
        TimingSub::Forced(candidates) => {
            // Advance our own cursor first (see the `Reaction`-resumes-correctly
            // note above), then fire the forced candidates the scan that pushed
            // this point found.
            set_timing_sub(cx, TimingSub::Reaction);
            if candidates.len() >= 2 {
                // 2+ forced: an ordered run (Rules Reference p.17). The run
                // carries no continuation — when it closes the loop re-dispatches the
                // parent `TimingPoint` (now at `Reaction`).
                reaction_windows::open_forced_resolution(cx, &event, bucket, candidates)
            } else if let Some(hit) = candidates.first() {
                forced_triggers::queue_forced_triggers(cx, &event, hit)
            } else {
                EngineOutcome::Done
            }
        }
        TimingSub::Reaction => {
            let candidates = trigger_scan::collect_reactions(cx.state, cx.cards, &event, bucket);
            if candidates.is_empty() {
                finish_timing_point(cx);
                EngineOutcome::Done
            } else {
                set_timing_sub(cx, TimingSub::Done);
                reaction_windows::open_reaction_run(cx, &event, bucket, candidates)
            }
        }
        TimingSub::Done => {
            finish_timing_point(cx);
            EngineOutcome::Done
        }
    }
}

/// Pop the finished [`Continuation::TimingPoint`] and advance the now-exposed
/// parent [`Continuation::EmitEvent`]'s bucket cursor.
fn finish_timing_point(cx: &mut Cx) {
    cx.state.continuations.pop_expect::<TimingPointFrame>();
    advance_or_finish_emit(cx);
}

/// Advance the top [`Continuation::EmitEvent`]'s cursor `When →
/// ResolveCondition → At → After`, or pop the coordinator once `After` is done.
fn advance_or_finish_emit(cx: &mut Cx) {
    let emit = cx.state.continuations.top_mut::<EmitEventFrame>();
    match emit.step.next() {
        Some(next) => emit.step = next,
        None => {
            cx.state.continuations.pop_expect::<EmitEventFrame>();
        }
    }
}

/// Whether the condition the frame on top belongs to was prevented in its
/// `when` cell: the [`EmitEventFrame::prevented`] flag of the nearest
/// [`Continuation::EmitEvent`] on the
/// stack. A [`Continuation::TimingPoint`]
/// is pushed straight above its condition's `EmitEvent`, and a window straight
/// above its cell's `TimingPoint`, so for either on top the nearest one down is
/// its own condition. The one reading both the coordinator and the window's
/// withdrawal share.
pub(super) fn condition_is_prevented(cx: &Cx) -> bool {
    cx.state
        .continuations
        .topmost_of::<EmitEventFrame>()
        .is_some_and(|condition| condition.prevented)
}

/// Pop the [`Continuation::EmitEvent`] coordinator on top **without** walking
/// the rest of its sequence — the prevented condition's exit. The
/// remaining cells are not visited at all, so nothing is left for a later
/// re-dispatch to pick up.
fn abandon_emit(cx: &mut Cx) {
    // A checked pop rather than a debug-only assertion: the pop is
    // unconditional, so a violated invariant would silently discard *someone
    // else's* frame in a release build.
    cx.state.continuations.pop_expect::<EmitEventFrame>();
}

/// A resolve step voids the rest of **its own** sequence: the `at` and `after`
/// cells of the condition being resolved never run. It is the exit a
/// `when`-cell cancel takes ([`abandon_emit`]), taken from inside step 2.
///
/// `SkillTestResolved` is the one caller. Its resolve step re-totals the test
/// and, when a `when`-cell modifier (Lucky! 01080) has turned the announced
/// verdict into the other one, the stale verdict's remaining cells must not see
/// it — the fresh verdict is announced in a sequence of its own.
///
/// Callable only from a resolve step that has pushed nothing yet: the
/// coordinator advanced its own frame to `At` just before calling it, so that
/// frame is the one on top, and the checked pop enforces it.
pub(super) fn void_remaining_sequence(cx: &mut Cx) {
    debug_assert!(
        cx.state
            .continuations
            .top_of::<EmitEventFrame>()
            .map(|frame| frame.step)
            == Some(EmitStep::At),
        "void_remaining_sequence: called outside a resolve step, or after it pushed a frame"
    );
    abandon_emit(cx);
}

/// Park the coordinator on top, whose cursor stands at `At`, on the effect it
/// arose from, if it arose from one: move it into the `parked` list of the
/// deepest [`EffectBaseFrame`] above the nearest boundary, and return `true`.
/// With no effect base above the boundary, nothing parks and this returns
/// `false`.
///
/// The `at` and `after` cells of a condition an effect raised wait until the
/// whole effect has resolved. `glossary/Effects.md`: *"All aspects of an effect
/// have timing priority over all "after..." triggering conditions that might
/// arise as a consequence of that effect."* `glossary/Then.md` says the same of
/// a nested effect: *"The post-then aspect of an effect has timing priority over
/// all other indirect consequences of the resolution of the pre-then aspect."*
/// That is why the **deepest** base is the target: a Revelation drawn by Drawn
/// to the Flame 01064 is an effect nested in the play, and the play's *"Then,
/// discover 2 clues at your location."* lands before the consequences of the
/// Revelation. The `at` cell waits with the `after` cell (ADR 0021).
///
/// The search runs down from the frame beneath the coordinator. Every frame
/// other than a boundary is transparent, including a coordinator in the middle
/// of resolving its own condition: `DamagePlaced`, raised while `DamageDealt`
/// resolves, parks to the base `DamageDealt` would park to. The boundaries are
/// in [`bounds_effect_deferral`].
///
/// Called only at `At` with the coordinator on top, before the `at` cell is
/// scanned, so no [`TimingPointFrame`] of the condition's own is ever separated
/// from it. Parking happens at that point rather than when the condition is
/// raised, so released windows open in the order each condition reached its
/// `at` cell.
fn park_in_effect_base(cx: &mut Cx) -> bool {
    let beneath_top = cx.state.continuations.len().saturating_sub(1);
    let mut deepest_base = None;
    for (index, frame) in cx
        .state
        .continuations
        .iter()
        .enumerate()
        .take(beneath_top)
        .rev()
    {
        if bounds_effect_deferral(frame) {
            break;
        }
        if matches!(frame, Continuation::EffectBase(_)) {
            deepest_base = Some(index);
        }
    }
    let Some(base) = deepest_base else {
        return false;
    };
    let coordinator = cx.state.continuations.pop_expect::<EmitEventFrame>();
    let Some(Continuation::EffectBase(EffectBaseFrame { parked })) =
        cx.state.continuations.frames_mut().nth(base)
    else {
        unreachable!("park_in_effect_base: the frame the search found is an effect base");
    };
    parked.push(coordinator);
    true
}

/// Whether `frame` bounds the search for the effect a condition parks on: a
/// condition raised above it is a consequence of the procedure the frame runs,
/// not of an effect beneath it.
///
/// - **A timing cell** ([`TimingPointFrame`] and the window above one). An
///   ability initiated from a cell starts a nested sequence, which completes
///   before the game returns to the sequence that spawned it.
///   `glossary/Nested_Sequences.md`: *"Within this sequence, if the use of a
///   \[reaction\] or **Forced** ability leads to a new triggering condition, the
///   game pauses and starts a new sequence"*. Its Guard Dog example resolves
///   Agnes Baker's reaction to the Goat Spawn's horror *"Before resolving the
///   Goat Spawn's defeat"*.
/// - **A skill test.** A condition raised while the test resolves opens its
///   cells inside the test. The Official FAQ on Shrivelling 01060's horror
///   (`data/official-faq/Frequently_Asked_Questions.md`): *"If you had any
///   reactions to taking that horror (such as Agnes Baker’s ability), you would
///   trigger it then, before resolving the rest of the attack."*
/// - **An act or agenda advance.** `glossary/Act_Deck_and_Agenda_Deck.md`
///   step 2, *"Flip the advancing card over and follow the instructions on the
///   reverse ("b") side."*, makes the reverse a step of the advance, so it runs
///   before step 3 rather than after the effect that advanced the deck.
/// - **An elimination.** Its step-0 game-end abilities fire on weaknesses that
///   steps 1–6 then remove (ADR 0008), so they run before those steps.
fn bounds_effect_deferral(frame: &Continuation) -> bool {
    matches!(
        frame,
        Continuation::TimingPoint(_)
            | Continuation::TimingPointWindow(_)
            | Continuation::SkillTest(_)
            | Continuation::AdvanceReverse(_)
            | Continuation::Elimination(_)
    )
}

/// Set the top [`Continuation::TimingPoint`]'s `sub` cursor.
fn set_timing_sub(cx: &mut Cx, sub: TimingSub) {
    cx.state.continuations.top_mut::<TimingPointFrame>().sub = sub;
}
