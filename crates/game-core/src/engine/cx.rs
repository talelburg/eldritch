//! The engine's mutation context: the mutable working set threaded
//! through every dispatch handler and effect evaluation.
//!
//! `Cx` bundles the two `&mut` references that previously rode together
//! by hand through every signature — the [`GameState`] being mutated and
//! the [`Event`] buffer being emitted into. It is *not* a semantic
//! context: the "you"/"source" of card text lives in
//! [`EvalContext`](super::EvalContext), which travels alongside `Cx`
//! as a separate `eval_ctx` parameter in the evaluator.
//!
//! Bare field bundle by design — no helper methods. Read-only callees
//! keep taking `&GameState`; a holder of `cx` calls them as
//! `read_fn(cx.state, …)`, which borrows only `cx.state` and leaves
//! `cx.events` independently usable (disjoint-field borrowing).

use crate::event::Event;
use crate::state::GameState;

/// Mutable engine working set: the state being mutated plus the event
/// buffer being emitted into. Threaded as `cx: &mut Cx` through dispatch
/// handlers and the effect evaluator.
///
/// Public because it is the effect-resolution context passed to a
/// [`NativeEffectFn`](crate::card_registry::NativeEffectFn): a card-local
/// Rust effect mutates `state` and pushes `events` through it.
pub struct Cx<'a> {
    /// The game state being mutated.
    pub state: &'a mut GameState,
    /// The events emitted by the current `apply` call.
    pub events: &'a mut Vec<Event>,
}
