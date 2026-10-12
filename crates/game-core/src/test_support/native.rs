//! Calling a card-local native directly, outside the evaluator.

use crate::engine::evaluator::EvalContext;
use crate::engine::{Cx, EngineOutcome, NativeCx};
use crate::event::Event;
use crate::state::GameState;

/// Run `native` once against `state` in `eval_ctx`, returning its outcome and
/// the events it emitted. `state` keeps whatever the native wrote.
///
/// A direct call, for a unit test of one native's resolution: no apply, so no
/// rollback on `Rejected`, and none of the evaluator's handling of a suspend
/// (no re-push of the effect, no choose-before-write check). To test a native
/// as the game runs it, register it in a mock registry and drive it through a
/// [`TestSession`](crate::test_support::TestSession).
pub fn run_native(
    state: &mut GameState,
    native: impl FnOnce(&mut NativeCx, &EvalContext) -> EngineOutcome,
    eval_ctx: &EvalContext,
) -> (EngineOutcome, Vec<Event>) {
    let mut events = Vec::new();
    let mut cx = NativeCx::new(Cx {
        state,
        events: &mut events,
    });
    let outcome = native(&mut cx, eval_ctx);
    (outcome, events)
}
