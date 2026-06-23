//! The `when → at → after` timing-bucket coordinator frames (EmitEvent-frame
//! C-coordinators, #434).
//!
//! [`super::emit::emit_event`] pushes a [`Continuation::EmitEvent`] for the only
//! multi-bucket event (`RoundEnded`); the `drive` loop dispatches it here.
//!
//! - [`dispatch_emit_event`] walks `When → At → After`, pushing a
//!   [`Continuation::TimingPoint`] for each *populated* bucket and **re-scanning
//!   each cell fresh** (the per-cell eligibility re-scan — a `when` reaction can
//!   change whether an `at` forced fires; the grid is not pre-computed).
//! - [`dispatch_timing_point`] resolves one bucket's forced-then-reaction
//!   (`sub` cursor `Forced → Reaction → Done`) — what single-bucket
//!   `emit_event` does today, made frame-resumable.
//!
//! Neither driver suspends *itself*: each does one step and returns `Done`, and
//! the loop re-dispatches the (mutated) top frame — or `AwaitingInput` when the
//! step opens a window / forced run. On a child's pop the parent's cursor has
//! already been advanced, so re-dispatch makes progress (never re-scans the same
//! cell into a loop).

use crate::dsl::EventTiming;
use crate::state::{Continuation, TimingSub};

use super::super::outcome::EngineOutcome;
use super::Cx;

/// Dispatch the [`Continuation::EmitEvent`] coordinator on top of the stack
/// (called only by the `drive` loop with one on top). Re-scans the current
/// bucket; if it has any forced or reaction ability, pushes a
/// [`Continuation::TimingPoint`] and yields; otherwise advances the cursor (or
/// pops the coordinator after `After`).
pub(super) fn dispatch_emit_event(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::EmitEvent { event, bucket }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("dispatch_emit_event: top frame is not EmitEvent");
    };
    // Per-cell re-scan (#434): the prior bucket may have changed board state.
    let has_forced = event.forced_point().is_some_and(|point| {
        !super::forced_triggers::collect_forced_hits(cx.state, &point, bucket).is_empty()
    });
    let has_reaction =
        !super::reaction_windows::scan_reactions_at(cx.state, &event, bucket).is_empty();
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
                // 2+ forced: the lead orders them (#213). Inside a coordinator
                // there is no framework tail — the parent `TimingPoint` (now at
                // `Reaction`) resumes when the run closes — so the run carries
                // `Terminal` (a no-op resume; the loop re-dispatches the parent).
                // `ForcedContinuation` is deleted in the follow-up slice (#434
                // Task 4), at which point the run carries no continuation at all.
                super::reaction_windows::open_forced_resolution(
                    cx,
                    &event,
                    candidates,
                    crate::state::ForcedContinuation::Terminal,
                )
            } else {
                super::forced_triggers::fire_forced_triggers(cx, &point, bucket)
            }
        }
        TimingSub::Reaction => {
            let candidates = super::reaction_windows::scan_reactions_at(cx.state, &event, bucket);
            if candidates.is_empty() {
                finish_timing_point(cx);
                EngineOutcome::Done
            } else {
                set_timing_sub(cx, TimingSub::Done);
                super::reaction_windows::open_reaction_run(cx, &event, candidates)
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

/// Advance the top [`Continuation::EmitEvent`]'s bucket cursor `When → At →
/// After`, or pop the coordinator once `After` is done.
fn advance_or_finish_emit(cx: &mut Cx) {
    let Some(Continuation::EmitEvent { bucket, .. }) = cx.state.continuations.last_mut() else {
        unreachable!("advance_or_finish_emit: expected an EmitEvent on top");
    };
    match *bucket {
        EventTiming::When => *bucket = EventTiming::At,
        EventTiming::At => *bucket = EventTiming::After,
        EventTiming::After => {
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
