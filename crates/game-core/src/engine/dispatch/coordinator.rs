//! The timing-sequence coordinator frames (EmitEvent-frame C-coordinators,
//! #434): the `when → at → after` cells of one triggering condition, with the
//! condition's own resolution between the first two (#701).
//!
//! [`super::emit::queue_event`] pushes a [`Continuation::EmitEvent`] for **every**
//! triggering condition (#702 — bar the three enemy-attack-machinery holdouts it
//! documents); the `drive` loop dispatches it here.
//!
//! - [`dispatch_emit_event`] walks `When → ResolveCondition → At → After` —
//!   the three RR timing cells with the triggering condition's own resolution
//!   between the first two (#701) — pushing a
//!   [`Continuation::TimingPoint`] for each *populated* bucket and **re-scanning
//!   each cell fresh** (the per-cell eligibility re-scan — a `when` reaction can
//!   change whether an `at` forced fires; the grid is not pre-computed).
//! - [`dispatch_timing_point`] resolves one cell's forced-then-reaction
//!   (`sub` cursor `Forced → Reaction → Done`) — RR p.2's forced-before-reaction,
//!   which since #702 is the *only* mechanism for that ordering (the pre-#702
//!   single-cell path achieved it structurally, by queueing the reaction window
//!   beneath the forced frames).
//!
//! Neither driver suspends *itself*: each does one step and returns `Done`, and
//! the loop re-dispatches the (mutated) top frame — or `AwaitingInput` when the
//! step opens a window / forced run. On a child's pop the parent's cursor has
//! already been advanced, so re-dispatch makes progress (never re-scans the same
//! cell into a loop).

use crate::state::{Continuation, EmitStep, TimingSub};

use super::super::outcome::EngineOutcome;
use super::emit::ConditionResolution;
use super::Cx;

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
/// A caller-owned condition's `when` cell is **not walked** — the caller has
/// already mutated, so an interrupt there would resolve after the thing it meant
/// to interrupt. An ability that declares one is rejected rather than dropped
/// (this project never silently no-ops); see [`ConditionResolution::Caller`] and
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
pub(super) fn dispatch_emit_event(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::EmitEvent { event, step }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("dispatch_emit_event: top frame is not EmitEvent");
    };
    let resolution = event.condition_resolution();
    if step == EmitStep::ResolveCondition {
        advance_or_finish_emit(cx);
        return match resolution {
            ConditionResolution::Coordinator(resolve) => resolve(cx, &event),
            // Already resolved, by the caller, before it emitted.
            ConditionResolution::Caller => EngineOutcome::Done,
        };
    }
    let Some(bucket) = step.cell() else {
        unreachable!("dispatch_emit_event: ResolveCondition handled above");
    };
    let caller_owned = matches!(resolution, ConditionResolution::Caller);
    // Per-cell re-scan (#434): the prior cell may have changed board state.
    let has_forced = event.forced_point().is_some_and(|point| {
        !super::forced_triggers::collect_forced_hits(cx.state, &point, bucket).is_empty()
    });
    let has_reaction =
        !super::reaction_windows::scan_reactions_at(cx.state, &event, bucket).is_empty();
    if caller_owned && step == EmitStep::When {
        if has_forced || has_reaction {
            return EngineOutcome::Rejected {
                reason: format!(
                    "timing coordinator: {event:?} is a caller-owned triggering condition, so its \
                     `when` cell is not walked — an ability declaring interrupt timing on it \
                     cannot resolve before the condition, which has already resolved. Migrate the \
                     condition to a coordinator-owned arm — TODO(#703/#704): the caller-owned \
                     arm is scaffolding, migrated one condition at a time (see \
                     docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md)."
                )
                .into(),
            };
        }
        advance_or_finish_emit(cx);
        return EngineOutcome::Done;
    }
    if has_forced || has_reaction {
        cx.state.continuations.push(Continuation::TimingPoint {
            event,
            bucket,
            sub: TimingSub::Forced,
        });
    } else {
        advance_or_finish_emit(cx);
    }
    EngineOutcome::Done
}

/// Dispatch the [`Continuation::TimingPoint`] on top of the stack (called only
/// by the `drive` loop). Runs the `sub` cursor `Forced → Reaction → Done`:
///
/// - **`Forced`** — fire the bucket's forced abilities (0/1 inline; 2+ via the
///   lead-ordered run). The cursor advances to `Reaction` *before* firing, so a
///   suspending 2+ run resumes at `Reaction`, not by re-scanning forced.
/// - **`Reaction`** — open the bucket's reaction window if any candidate; else
///   finish the bucket. The cursor advances to `Done` before opening, so the
///   re-dispatch after the window closes finishes (never re-opens).
/// - **`Done`** — advance the parent `EmitEvent`'s cursor and pop self.
pub(super) fn dispatch_timing_point(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::TimingPoint { event, bucket, sub }) =
        cx.state.continuations.last().cloned()
    else {
        unreachable!("dispatch_timing_point: top frame is not TimingPoint");
    };
    match sub {
        TimingSub::Forced => {
            // Advance our own cursor first (see the `Reaction`-resumes-correctly
            // note above), then fire forced.
            set_timing_sub(cx, TimingSub::Reaction);
            let Some(point) = event.forced_point() else {
                return EngineOutcome::Done;
            };
            let candidates = super::forced_triggers::collect_forced_hits(cx.state, &point, bucket);
            if candidates.len() >= 2 {
                // 2+ forced: the lead orders them (#213). The run carries no
                // continuation (#434) — when it closes the loop re-dispatches the
                // parent `TimingPoint` (now at `Reaction`).
                super::reaction_windows::open_forced_resolution(cx, &event, bucket, candidates)
            } else {
                super::forced_triggers::queue_forced_triggers(cx, &point, bucket)
            }
        }
        TimingSub::Reaction => {
            let candidates = super::reaction_windows::scan_reactions_at(cx.state, &event, bucket);
            if candidates.is_empty() {
                finish_timing_point(cx);
                EngineOutcome::Done
            } else {
                set_timing_sub(cx, TimingSub::Done);
                super::reaction_windows::open_reaction_run(cx, &event, bucket, candidates)
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
    let popped = cx.state.continuations.pop();
    debug_assert!(
        matches!(popped, Some(Continuation::TimingPoint { .. })),
        "finish_timing_point: expected a TimingPoint on top, popped {popped:?}",
    );
    advance_or_finish_emit(cx);
}

/// Advance the top [`Continuation::EmitEvent`]'s cursor `When →
/// ResolveCondition → At → After`, or pop the coordinator once `After` is done.
fn advance_or_finish_emit(cx: &mut Cx) {
    let Some(Continuation::EmitEvent { step, .. }) = cx.state.continuations.last_mut() else {
        unreachable!("advance_or_finish_emit: expected an EmitEvent on top");
    };
    match step.next() {
        Some(next) => *step = next,
        None => {
            cx.state.continuations.pop();
        }
    }
}

/// Set the top [`Continuation::TimingPoint`]'s `sub` cursor.
fn set_timing_sub(cx: &mut Cx, sub: TimingSub) {
    match cx.state.continuations.last_mut() {
        Some(Continuation::TimingPoint { sub: slot, .. }) => *slot = sub,
        other => unreachable!("set_timing_sub: expected a TimingPoint on top, got {other:?}"),
    }
}
