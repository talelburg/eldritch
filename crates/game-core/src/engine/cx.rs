//! The engine's mutation context: the mutable working set threaded
//! through every dispatch handler and effect evaluation.
//!
//! `Cx` bundles the two `&mut` references that previously rode together
//! by hand through every signature — the [`GameState`] being mutated and
//! the [`Event`] buffer being emitted into — plus the registries the
//! apply was handed: the card registry, and the scenario module resolved
//! once at the apply boundary. It is *not* a semantic
//! context: the "you"/"source" of card text lives in
//! [`EvalContext`](crate::engine::evaluator::EvalContext), which travels alongside `Cx`
//! as a separate `eval_ctx` parameter in the evaluator.
//!
//! Bare field bundle by design — no helper methods. Read-only callees
//! keep taking `&GameState`; a holder of `cx` calls them as
//! `read_fn(cx.state, …)`, which borrows only `cx.state` and leaves
//! `cx.events` independently usable (disjoint-field borrowing).
//!
//! Crate-private: a card-local native never holds one. It gets a
//! [`NativeCx`](crate::engine::NativeCx), which wraps a `Cx` and changes the
//! game only through the engine's exits.

use crate::card_registry::CardRegistry;
use crate::event::Event;
use crate::scenario::ScenarioModule;
use crate::state::GameState;

/// Mutable engine working set: the state being mutated plus the event
/// buffer being emitted into, and the registries this apply runs against.
/// Threaded as `cx: &mut Cx` through dispatch handlers and the effect
/// evaluator.
pub(crate) struct Cx<'a> {
    /// The game state being mutated.
    pub(crate) state: &'a mut GameState,
    /// The events emitted by the current `apply` call.
    pub(crate) events: &'a mut Vec<Event>,
    /// The card registry this apply was handed.
    pub(crate) cards: &'a CardRegistry,
    /// The active scenario's module, looked up once at the apply boundary
    /// from `state.scenario_id`. `None` when there is no scenario id, or the
    /// scenario registry doesn't know it.
    pub(crate) scenario: Option<&'static ScenarioModule>,
}

#[cfg(test)]
impl<'a> Cx<'a> {
    /// A unit test's `Cx`: game-core's
    /// [`TEST_REGISTRY`](crate::test_support::TEST_REGISTRY) and no scenario
    /// module. A test that needs another registry overrides the field.
    pub(crate) fn for_test(state: &'a mut GameState, events: &'a mut Vec<Event>) -> Self {
        Self {
            state,
            events,
            cards: &crate::test_support::TEST_REGISTRY,
            scenario: None,
        }
    }
}
