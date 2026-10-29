//! Calling a card-local native directly, outside the evaluator.

use card_dsl::dsl;

use crate::card_registry::CardRegistry;
use crate::engine::evaluator::EvalContext;
use crate::engine::{Cx, EngineOutcome, NativeCx};
use crate::event::Event;
use crate::state::GameState;

/// Run `native` once against `state` in `eval_ctx`, with `cards` as the card
/// registry [`NativeCx::cards`] answers with and no scenario module, returning
/// its outcome and the events it emitted.
///
/// `state` keeps whatever the native wrote. A native that suspends leaves its
/// prompt resting on an effect frame for a stand-in `Effect::Native` node
/// whose tag no registry knows, so the state can be read but not resumed.
///
/// A direct call, for a unit test of one native's resolution: no apply, so no
/// rollback on `Rejected`, and none of the evaluator's checks around a native
/// (no choose-before-write check, no refusal of a read after a deferral). Exits
/// deferred behind pending work are parked on the stack as the evaluator parks
/// them, but nothing drives them. To test a native
/// as the game runs it, register it in a mock registry and drive it through a
/// [`TestSession`](crate::test_support::TestSession).
pub fn run_native(
    state: &mut GameState,
    cards: &CardRegistry,
    native: impl FnOnce(&mut NativeCx, &EvalContext) -> EngineOutcome,
    eval_ctx: &EvalContext,
) -> (EngineOutcome, Vec<Event>) {
    let mut events = Vec::new();
    let node = dsl::native("test_support::run_native");
    let mut cx = NativeCx::new(
        Cx {
            state,
            events: &mut events,
            cards,
            scenario: None,
        },
        &node,
        *eval_ctx,
    );
    let outcome = native(&mut cx, eval_ctx);
    cx.park_tail();
    (outcome, events)
}
