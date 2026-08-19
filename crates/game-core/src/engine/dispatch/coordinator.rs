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
//! - A `when`-cell ability that **prevents** the condition suppresses the rest
//!   of the sequence — the resolve step, the `at` and `after` cells, and the
//!   remainder of the `when` cell itself (#714; citations on
//!   [`prevented_in_the_when_cell`]).
//!
//! Neither driver suspends *itself*: each does one step and returns `Done`, and
//! the loop re-dispatches the (mutated) top frame — or `AwaitingInput` when the
//! step opens a window / forced run. On a child's pop the parent's cursor has
//! already been advanced, so re-dispatch makes progress (never re-scans the same
//! cell into a loop).

use crate::dsl::EventTiming;
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
///
/// Visible to `engine` rather than to `dispatch` alone because the evaluator's
/// bounded test driver has to walk the coordinator too: since #703 an effect
/// leaf can emit a condition whose own resolution happens here, so a driver that
/// stopped at this frame would leave the effect half-resolved.
pub(in crate::engine) fn dispatch_emit_event(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::EmitEvent { event, step }) = cx.state.continuations.last().cloned()
    else {
        unreachable!("dispatch_emit_event: top frame is not EmitEvent");
    };
    let resolution = event.condition_resolution();
    if step == EmitStep::ResolveCondition {
        return match resolution {
            ConditionResolution::Coordinator(resolve) => {
                if prevented_in_the_when_cell(cx) {
                    // A `when`-cell ability prevented the condition, so neither
                    // step 2 nor the cells after it happen: the sequence is
                    // abandoned where it stands (#714, and see
                    // [`prevented_in_the_when_cell`] for the citations).
                    abandon_emit(cx);
                    EngineOutcome::Done
                } else {
                    // Advance *before* resolving, so a resolution that suspends
                    // resumes at the `at` cell rather than resolving twice.
                    advance_or_finish_emit(cx);
                    resolve(cx, &event)
                }
            }
            // Already resolved, by the caller, before it emitted. The caller
            // also owns the cancellation signal (`combat::resume_enemy_attack`
            // reads it on its own resume), so it is left untouched here.
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
                     condition to a coordinator-owned arm — TODO(#704): the caller-owned \
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
///
/// Visible to `engine` for the same reason as [`dispatch_emit_event`].
pub(in crate::engine) fn dispatch_timing_point(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::TimingPoint { event, bucket, sub }) =
        cx.state.continuations.last().cloned()
    else {
        unreachable!("dispatch_timing_point: top frame is not TimingPoint");
    };
    if bucket == EventTiming::When && cx.state.pending_cancellation {
        // A `when`-cell ability just prevented the condition. The rest of *this*
        // cell is suppressed along with the cells after it (#714) — Dodge
        // 01023's ruling reaches the `when` cell itself. Any candidate still
        // pending in an open window of this cell was withdrawn by
        // `reaction_windows::withdraw_suppressed_candidates` before that window
        // closed; this is the cursor half, skipping the sub-steps not yet
        // started. Finishing the point hands back to the coordinator, which
        // reads the same signal at its resolve step and abandons the sequence.
        //
        // No ownership check is needed: a caller-owned condition's `when` cell is
        // never walked (the arm above returns before any `TimingPoint` is
        // pushed), so a `When` bucket here is coordinator-owned by construction.
        // The signal names no condition, though — it is one global bool while
        // Before-windows cannot nest (TODO(#367)) — so a condition emitted while
        // it is live would consume it and skip the wrong cell. Unreachable today:
        // the only setter, `Effect::Cancel`, is terminal in every card that uses
        // it, and one cancellable impact is in flight at a time.
        finish_timing_point(cx);
        return EngineOutcome::Done;
    }
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

/// Read **and clear** the prevention signal an
/// [`Effect::Cancel`](crate::dsl::Effect::Cancel) in the `when` cell set (Cover
/// Up 01007's *"…instead"*, #336).
///
/// Read at the resolve step rather than at the window's close: the check belongs
/// between the `when` cell and step 2, which is where the sequence puts the
/// chance to prevent the condition from resolving.
///
/// A `true` here suppresses **the whole rest of the sequence**, not just step 2
/// (#714). Dodge 01023's ruling, `data/arkhamdb-faq/core/01023.md`, verbatim:
///
/// > If the attacking enemy has a **Forced** ability that says "When attacks" or
/// > "After attacks", that ability does not trigger if an attack is Dodged.
///
/// `glossary/After.md` agrees from the other side — after is *"the moment
/// immediately after the specified timing point or triggering condition **has
/// fully resolved**"*, and a prevented condition never fully resolves — as does
/// `glossary/Instead.md` for the nature-changing case: *"If a replacement effect
/// that uses the word "would" changes the nature of a triggering condition, the
/// original triggering condition is replaced with the new triggering condition.
/// No further abilities referencing the original triggering condition may be
/// used."* Cover Up 01007 (*"When you would discover 1 or more clues at your
/// location: Discard that many clues from Cover Up instead."*) is exactly that.
///
/// **One signal, two suppressing arms.** A cancel (`glossary/Cancel.md`: *"Cancel
/// abilities interrupt the initiation of an effect, and prevent the effect from
/// initiating."*) and a nature-changing replacement suppress identically, so the
/// engine does not distinguish them. The third arm — a replacement that resolves
/// the condition *by other means*, whose later cells presumably do still run —
/// is **not** modelled: no corpus card wants one, and it is already recorded as
/// #366 (replace-with-different-impact). See ADR 0008.
///
/// The clear is **every** coordinator-owned arm's, not this or that condition's
/// — including one whose resolve step ignores the value, like the round end's
/// bare milestone. That is deliberate: an unconsumed signal would otherwise
/// reach the next condition's resolve step and cancel the wrong thing. A
/// caller-owned condition is untouched, because its emit site does its own
/// read-and-clear (`combat::resume_enemy_attack`).
fn prevented_in_the_when_cell(cx: &mut Cx) -> bool {
    std::mem::take(&mut cx.state.pending_cancellation)
}

/// Pop the [`Continuation::EmitEvent`] coordinator on top **without** walking
/// the rest of its sequence — the prevented condition's exit (#714). The
/// remaining cells are not visited at all, so nothing is left for a later
/// re-dispatch to pick up.
fn abandon_emit(cx: &mut Cx) {
    // `unreachable!` rather than a debug-only assertion: the pop is unconditional,
    // so a violated invariant would silently discard *someone else's* frame in a
    // release build. Matches this file's other invariant checks.
    let Some(Continuation::EmitEvent { .. }) = cx.state.continuations.pop() else {
        unreachable!("abandon_emit: expected an EmitEvent on top of the stack");
    };
}

/// Set the top [`Continuation::TimingPoint`]'s `sub` cursor.
fn set_timing_sub(cx: &mut Cx, sub: TimingSub) {
    match cx.state.continuations.last_mut() {
        Some(Continuation::TimingPoint { sub: slot, .. }) => *slot = sub,
        other => unreachable!("set_timing_sub: expected a TimingPoint on top, got {other:?}"),
    }
}
