//! `Condition::Native` dispatch (#592): a card's `native_condition(tag)`
//! predicate resolves through `CardRegistry.native_condition_for` to a
//! host-provided `fn(&GameState, &EvalContext) -> bool`, and gates the
//! surrounding effect on its verdict.
//!
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)` — same shape as `native_effect.rs`.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{ApplyResult, EngineOutcome, TimingEvent};
use game_core::state::{self, Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{self, MockRegistry, TestSession};

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";
const INV: InvestigatorId = InvestigatorId(1);

/// Forced at end of the enemy phase: gain 2 resources when the native
/// predicate holds, 5 when it does not — two distinct observable branches.
fn gated(tag: &'static str) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        dsl::if_else(
            dsl::native_condition(tag),
            dsl::gain_resources(InvestigatorTarget::You, 2),
            dsl::gain_resources(InvestigatorTarget::You, 5),
        ),
    )]
}

/// Reads both arguments the predicate signature provides: board state and the
/// evaluation context's controller.
fn has_doom(state: &GameState, ctx: &EvalContext) -> bool {
    state.agenda_doom > 0 && state.investigators.contains_key(&ctx.controller)
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(AGENDA, || gated("test:has-doom"))
        .with_abilities(AGENDA_BAD, || gated("test:missing"))
        .with_native_condition("test:has-doom", has_doom)
        .install();
}

fn state_with_agenda(code: &str, doom: u8) -> GameState {
    // `turn_order` must hold an Active investigator: `PhaseEnded` forced
    // dispatch binds the controller to the first one and returns no hits
    // otherwise.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([INV])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state.agenda_doom = doom;
    state
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

#[test]
fn native_condition_holding_takes_the_then_branch() {
    let state = state_with_agenda(AGENDA, 1);
    let before = resources(&state);
    let ApplyResult { state, outcome, .. } = TestSession::new(state)
        .fire_at(TimingEvent::PhaseEnded {
            phase: state::Phase::Enemy,
        })
        .finish();
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(resources(&state), before + 2, "predicate true → `then`");
}

#[test]
fn native_condition_failing_takes_the_else_branch() {
    let state = state_with_agenda(AGENDA, 0);
    let before = resources(&state);
    let ApplyResult { state, outcome, .. } = TestSession::new(state)
        .fire_at(TimingEvent::PhaseEnded {
            phase: state::Phase::Enemy,
        })
        .finish();
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(resources(&state), before + 5, "predicate false → `else_`");
}

/// An unregistered tag is a card-authoring bug, not a `false` verdict — it
/// rejects loudly rather than silently taking the `else_` branch.
#[test]
fn native_condition_rejects_unknown_tag() {
    let state = state_with_agenda(AGENDA_BAD, 1);
    let before = resources(&state);
    let ApplyResult { state, outcome, .. } = TestSession::new(state)
        .fire_at(TimingEvent::PhaseEnded {
            phase: state::Phase::Enemy,
        })
        .finish();
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "unknown tag rejects; got {outcome:?}"
    );
    assert_eq!(resources(&state), before, "no mutation on reject");
}
