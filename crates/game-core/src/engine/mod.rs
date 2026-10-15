//! The engine: applies actions to game state, emits events.
//!
//! The central function here is [`apply`], which takes the current
//! state plus an [`Action`] and returns an [`ApplyResult`] containing
//! the new state, the events emitted, and an [`EngineOutcome`]
//! summarizing what happened.
//!
//! State changes happen exclusively through this function. The action
//! log persisted by the server is a flat sequence of [`Action`]s;
//! replaying it via [`apply`] from the initial state reproduces the
//! current state bit-for-bit.

pub(crate) mod abilities_in_effect;
pub(crate) mod ability_source;
pub mod board;
mod cx;
pub(crate) use cx::Cx;
pub(crate) mod designator;
mod dispatch;
pub mod enumerate;
pub mod evaluator;
pub mod modified_value;
mod native_cx;
pub use native_cx::NativeCx;
mod outcome;
pub(crate) mod pathfinding;

// The read-only helpers a card-local native calls on `NativeCx::state`. The
// exits that change the game are crate-private: natives reach them as
// `NativeCx` methods.
pub use board::MoveResourcesError;
pub use dispatch::act_agenda::round_end_advance_affordable;
pub use dispatch::choice::{resolve_card_choice, resolve_grounded_choice, Grounded};
pub use dispatch::movement::{enemy_can_enter_location, investigator_can_enter_location};
pub use dispatch::uses::{MoveUsesError, SpendUsesError};
// `pub(crate)`: `test_support`'s session deals damage outside any attack.
pub(crate) use dispatch::combat::deal_harm;
pub use enumerate::{legal_actions, TurnAction};
pub use evaluator::{location_id_by_code, EvalContext};
pub use modified_value::{
    modified_value, Contribution, ContributionSource, ModifiedQuantity, ModifierBreakdown,
    ModifierTarget, ReadContext,
};
pub use outcome::{
    ChoiceOption, EngineOutcome, InputKind, InputRequest, OptionId, OptionTarget, PromptNature,
    ResumeToken,
};
pub use pathfinding::shortest_first_steps;

// The unified trigger-dispatch chokepoint's key (Axis-B T5a).
pub use dispatch::emit::TimingEvent;
// `pub(crate)`: `apply` routes player actions through `apply_player_action`, and
// `test_support`'s session drives turn actions and the loop directly.
pub(crate) use dispatch::{apply_player_action, dispatch_turn_action, drive};
// `pub(crate)` so `TestSession::test_skill` can start a plain skill test
// directly (the synthetic entry point that replaced the retired
// `PlayerAction::PerformSkillTest` wire variant, #447).
pub(crate) use dispatch::skill_test::perform_skill_test as start_plain_skill_test;

use card_dsl::card_data::CardKind;

use crate::action::{Action, RosterEntry};
use crate::card_registry::CardRegistry;
use crate::event::Event;
use crate::scenario::ScenarioRegistry;
use crate::state::{CardCode, Continuation, GameState, ScenarioEndFrame, ScenarioEndStep};

/// The card and scenario registries one [`apply`] runs against. The host
/// hands them in; the engine carries them on its `Cx` for the length of the
/// apply, looking the scenario module up once from `state.scenario_id`.
///
/// `Copy`: both halves are shared references to fn-pointer bundles, and
/// `'static` so a value can be stored on a builder or a host's state.
#[derive(Debug, Clone, Copy)]
pub struct Registries {
    /// The card registry: metadata, abilities, natives.
    pub cards: &'static CardRegistry,
    /// The scenario registry: modules by scenario id.
    pub scenarios: &'static ScenarioRegistry,
}

/// The result of a single [`apply`] call.
#[derive(Debug, Clone)]
#[must_use = "the post-apply GameState lives in ApplyResult.state; dropping the result drops the new state"]
#[non_exhaustive]
pub struct ApplyResult {
    /// The state after the action was applied. If the action was
    /// rejected, this is unchanged from the input state.
    pub state: GameState,
    /// Events emitted by the action's resolution. Empty if rejected.
    pub events: Vec<Event>,
    /// The terminal outcome of this apply call.
    pub outcome: EngineOutcome,
}

/// Apply a single action to the state.
///
/// Returns an [`ApplyResult`] containing the new state, events emitted,
/// and an [`EngineOutcome`] summarizing the result.
///
/// `apply` is the only entry point for state mutation; all changes flow
/// through here. It must be deterministic — same input state and
/// action always produce the same output — so the action log replays
/// cleanly.
///
/// # Handler contract
///
/// On [`EngineOutcome::Rejected`], the returned state and event list
/// are unchanged from the input. `apply` enforces this **structurally**:
/// it snapshots the state before dispatch and restores the snapshot on
/// rejection, and clears the (per-apply) event buffer. So no handler —
/// including the fallible-and-mutating DSL evaluator — can leak partial
/// state on rejection; handlers need not be defensively validate-first
/// for *correctness* of this invariant (they still should be, for clear
/// rejection messages and to avoid wasted work).
///
/// The transaction boundary is the `apply` *call*, not a multi-call
/// logical action: a reject during a
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) rewinds to
/// the [`AwaitingInput`](EngineOutcome::AwaitingInput) pause state (the
/// input to that `apply`), not to before the original action — the pause
/// state was the product of an apply that returned `AwaitingInput`, whose
/// partial state is legitimate and retained.
///
/// On [`EngineOutcome::AwaitingInput`], the returned state and event
/// list reflect the work done up to the pause point — e.g. a skill test
/// that suspends at the commit window has
/// already emitted [`Event::SkillTestStarted`] and pushed the
/// [`SkillTest`](crate::state::Continuation::SkillTest) frame (read via
/// [`GameState::current_skill_test`]). The resume action
/// ([`PlayerAction::ResolveInput`](crate::action::PlayerAction::ResolveInput))
/// drives the rest of resolution in a subsequent `apply` call. While
/// paused, every non-`ResolveInput` player action rejects.
///
/// Runs against the [`Registries`] it is handed. The post-apply scenario
/// resolution hook: a `Rejected` outcome clears events and skips it; any
/// non-`Rejected` outcome (`Done` or `AwaitingInput`) fires it iff this apply
/// left the scenario's
/// [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd) frame on top at its
/// finalize step — i.e. the resolution latched *and* the game-end Forced
/// abilities it queued have finished (#566).
pub fn apply(state: GameState, action: Action, registries: Registries) -> ApplyResult {
    apply_via(state, registries, |cx| match action {
        Action::Player(p) => apply_player_action(cx, &p),
        Action::Engine(e) => dispatch::apply_engine_record(cx, &e),
    })
}

/// Create a freshly seated game: run scenario setup's roster seating over
/// `setup_state` and drive to the first `AwaitingInput` (the setup mulligan).
///
/// This is the non-logged seating path (#459). The returned
/// [`ApplyResult::state`] is already seated, shuffled, and mulligan-pending —
/// hosts persist it as the seed, so the action log is `ResolveInput`-only and
/// replay never re-runs setup RNG. Validation mirrors a player action: an
/// empty roster, an unknown/non-investigator code, or an already-started
/// state rejects with state unchanged.
pub fn seat_and_open(
    setup_state: GameState,
    roster: &[RosterEntry],
    registries: Registries,
) -> ApplyResult {
    apply_via(setup_state, registries, |cx| {
        dispatch::seat_and_open(cx, roster)
    })
}

/// The shared `apply` scaffolding, parameterized by the dispatch step.
///
/// Builds the [`Cx`] — resolving the scenario module once, from
/// `state.scenario_id` — runs `dispatch` to produce the outcome, then applies the
/// transactional-restore + resolution-hook contract uniformly:
/// [`apply`] passes the typed-action dispatch;
/// [`TestSession::submit_off_menu`](crate::test_support::TestSession::submit_off_menu)
/// passes a [`TurnAction`]-through-handler dispatch that bypasses the
/// enumeration gate (so handler-level corruption panics are reachable from
/// tests). Factored out so neither caller copy-pastes the `Cx` build, the
/// snapshot/restore, or the `None`->`Some` resolution-latch firing.
pub(crate) fn apply_via(
    state: GameState,
    registries: Registries,
    dispatch: impl FnOnce(&mut Cx) -> EngineOutcome,
) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    // `scenario_id` is set only when the state is built, never during
    // dispatch, so one lookup here serves the whole apply.
    let scenario = state
        .scenario_id
        .as_ref()
        .and_then(|id| (registries.scenarios.module_for)(id));
    // Transactional snapshot: a Rejected outcome must leave the returned
    // state byte-identical to the input (the engine's "Rejected => state
    // unchanged" contract). Taken before any handler runs and restored
    // below if the outcome is Rejected, so no handler — including the
    // fallible-and-mutating DSL evaluator — can leak partial state on
    // rejection. AwaitingInput is untouched: it legitimately returns the
    // work done up to the pause point, so we restore on Rejected only.
    //
    // RNG state (`state.rng`) is part of the snapshot, so a rejected
    // action that advanced the RNG is rewound too. That's correct for
    // replay: a rejected action contributes nothing to the action log, so
    // it must contribute no RNG consumption either.
    let pristine = state.clone();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
            cards: registries.cards,
            scenario,
        };
        let outcome = dispatch(&mut cx);
        if matches!(outcome, EngineOutcome::Rejected { .. }) {
            // Transactional restore (event half): the events buffer is
            // per-apply and starts empty, so clearing it == restoring it.
            // State half is restored after this block (the `cx` borrow on
            // `state` releases at the block close).
            cx.events.clear();
        } else {
            // The scenario's ending may have finished during this apply. Runs on
            // Done AND AwaitingInput: the ending itself never awaits input, but
            // an unrelated suspension can be surfaced by the same apply that
            // finalized nothing, and the check is cheap and self-guarding.
            finalize_scenario_end(&mut cx);
            // The rest invariant (#938): `Done` means the game is over, so the
            // stack is empty; `AwaitingInput` means a prompt is on top for the
            // next `apply` to answer. It holds because every prompt frame's push
            // site returns `AwaitingInput`, and every resume handler pops its
            // frame before doing more work — so a prompt is never exposed
            // without being surfaced, and nothing else may rest. A `Done` over a
            // non-empty stack is a frame nothing will ever advance, or a prompt
            // nobody was asked.
            debug_assert!(
                match outcome {
                    EngineOutcome::Done => cx.state.continuations.is_empty(),
                    _ => cx
                        .state
                        .continuations
                        .top()
                        .is_some_and(Continuation::awaits_input),
                },
                "`apply` returned {outcome:?} with {:?} on top of the continuation \
                 stack: `Done` must leave it empty, `AwaitingInput` must leave a \
                 prompt on top",
                cx.state.continuations.top(),
            );
        }
        outcome
        // `cx` drops here, releasing borrows on `state` and `events`.
    };
    // State half of the transactional restore: now that `cx`'s borrow on
    // `state` is released, swap the (possibly partially-mutated) state
    // back to the pristine snapshot on rejection.
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        state = pristine;
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
}

/// Post-dispatch hook: finish the scenario's ending if the `drive` loop left it
/// ready (#566).
///
/// The ending is a [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd)
/// frame, pushed at the bottom of the continuation stack when the resolution
/// latched. The loop exposes it once every frame above has completed or been
/// cancelled, emits [`GameEnd`](TimingEvent::GameEnd) from it, and lets the
/// game-end Forced abilities — Cover Up 01007's mental trauma, with its
/// interactive acknowledge — drain above it, across as many `apply` calls as
/// that takes. Only when the frame is exposed again at
/// [`ScenarioEndStep::Finalize`](crate::state::ScenarioEndStep::Finalize) does
/// the ending finish, and it finishes **here**, at the apply boundary, reading
/// the scenario module the apply resolved onto `cx`.
///
/// Short-circuits on every other apply. Popping the frame is what makes this
/// fire-once: it is pushed exactly once (`end_scenario` is
/// first-writer-wins) and popped exactly once, so no "already finalized" flag
/// is needed. The `ScenarioResolved` event is a property of engine state, so it
/// fires even when no module is registered (or `scenario_id` is `None`); only
/// `apply_resolution` needs the module.
fn finalize_scenario_end(cx: &mut Cx) {
    if !matches!(
        cx.state.continuations.top(),
        Some(Continuation::ScenarioEnd(ScenarioEndFrame {
            step: ScenarioEndStep::Finalize,
        }))
    ) {
        return;
    }
    cx.state.continuations.pop_expect::<ScenarioEndFrame>();
    let Some(ending) = cx.state.ending else {
        debug_assert!(
            false,
            "a ScenarioEnd frame exists without a latched scenario ending; \
             `end_scenario` pushes the two together"
        );
        return;
    };
    cx.events.push(Event::ScenarioResolved { ending });

    // Place victory-point locations in the victory display. Runs BEFORE
    // `(module.apply_resolution)(...)` so the scan captures board state
    // at the moment the resolution latches, before any post-resolution
    // cleanup (apply_resolution, Phase 9) runs. Generic across scenarios;
    // reads victory values from the card registry; a code it doesn't know
    // has no victory value and is not placed.
    //
    // RR p.21: "At the end of a scenario, place each victory point
    // location that is in play, revealed, and with no clues on it in the
    // victory display."
    let card_reg = cx.cards;
    let placed: Vec<(CardCode, u8)> = cx
        .state
        .locations
        .values()
        .filter(|loc| loc.revealed && loc.clues == 0)
        .filter_map(|loc| {
            let meta = (card_reg.metadata_for)(&loc.code)?;
            match meta.kind {
                CardKind::Location {
                    victory: Some(v), ..
                } if v > 0 => Some((loc.code.clone(), v)),
                _ => None,
            }
        })
        .collect();
    for (code, victory) in placed {
        cx.state.victory_display.push(code.clone());
        cx.events
            .push(Event::EnteredVictoryDisplay { code, victory });
    }

    let Some(module) = cx.scenario else {
        return;
    };
    (module.apply_resolution)(ending, cx.state, cx.events);
}

#[cfg(test)]
mod tests;
