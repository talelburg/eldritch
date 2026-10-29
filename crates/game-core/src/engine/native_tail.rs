//! A native's tail: the exits it called after an earlier exit left work
//! pending, deferred until that work has resolved, so a native resolves in
//! the order its author wrote it.
//!
//! See `docs/adr/0020-a-native-resolves-in-program-order.md`.

use crate::engine::dispatch::emit::ConditionResolution;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{Continuation, GameState, NativeTailFrame};

/// The stack as it stood before a run of exits, so the frames they push can be
/// told from the ones beneath.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StackMark {
    len: usize,
    ending_latched: bool,
}

impl StackMark {
    /// Mark the stack of `state` as it stands.
    pub(crate) fn of(state: &GameState) -> Self {
        Self {
            len: state.continuations.len(),
            ending_latched: state.ending.is_some(),
        }
    }

    /// The stack index of the first frame pushed since the mark that starts a
    /// deferral, if any: the frame a deferred tail parks beneath.
    ///
    /// Every pushed frame starts it except the coordinator of a caller-owned
    /// condition (`EnemyDefeated` behind a lethal
    /// [`deal_damage_to_enemy`](crate::engine::NativeCx::deal_damage_to_enemy)):
    /// the caller has already made that change, so the frame holds only the
    /// condition's timing cells, and work called after it is still part of
    /// the effect that has priority over them (`glossary/Effects.md`).
    ///
    /// An ending latched since the mark was inserted at the bottom of the
    /// stack (ADR 0004), lifting every frame by one; the pushed frames are
    /// counted above it.
    pub(crate) fn deferral_start(self, state: &GameState) -> Option<usize> {
        let inserted = usize::from(!self.ending_latched && state.ending.is_some());
        state
            .continuations
            .iter()
            .enumerate()
            .skip(self.len + inserted)
            .find(|(_, frame)| starts_deferral(frame))
            .map(|(index, _)| index)
    }
}

/// Whether a frame an exit pushed defers the exits called after it.
fn starts_deferral(frame: &Continuation) -> bool {
    !matches!(
        frame,
        Continuation::EmitEvent(emit)
            if matches!(emit.event.condition_resolution(), ConditionResolution::Caller)
    )
}

/// Step the [`NativeTail`](Continuation::NativeTail) frame on top: run its ops
/// in order until one leaves work pending, then re-park the rest beneath that
/// work. `Rejected` naming the native if an op is.
pub(crate) fn drive_native_tail(cx: &mut Cx) -> EngineOutcome {
    let NativeTailFrame {
        native,
        ops,
        mut next,
    } = cx.state.continuations.pop_expect();
    let mark = StackMark::of(cx.state);
    while let Some(op) = ops.get(next).cloned() {
        next += 1;
        let outcome = op.run(cx);
        if let EngineOutcome::Rejected { reason } = outcome {
            return EngineOutcome::Rejected {
                reason: format!("Native effect {native:?}, a deferred exit: {reason}").into(),
            };
        }
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "an op never suspends: {outcome:?}"
        );
        if let Some(at) = mark.deferral_start(cx.state) {
            if next < ops.len() {
                cx.state
                    .continuations
                    .insert_beneath(at, NativeTailFrame { native, ops, next });
            }
            return EngineOutcome::Done;
        }
    }
    EngineOutcome::Done
}
