//! `Effect::Native` dispatch: a card's `native(tag)` effect resolves
//! through `CardRegistry.native_effect_for` to a host-provided Rust fn.
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)`.

use card_dsl::dsl::{self, forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{Cx, EngineOutcome};
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{self, GameStateBuilder, MockRegistry};

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";

/// Forced at end of enemy phase -> the native effect tagged `tag`.
fn forced_native(tag: &'static str) -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        native(tag),
    )]
}

fn set_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 7;
    EngineOutcome::Done
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(AGENDA, || forced_native("test:set-doom"))
        .with_abilities(AGENDA_BAD, || forced_native("test:missing"))
        .with_native_effect("test:set-doom", set_doom)
        .install();
}

fn state_with_agenda(code: &str) -> GameState {
    // `turn_order` must be non-empty: `PhaseEnded` forced dispatch binds
    // the controller to `turn_order.first()` and returns no hits otherwise.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn native_effect_runs_via_registry() {
    let mut state = state_with_agenda(AGENDA);
    let mut events = Vec::new();
    let outcome = test_support::fire_forced_on_phase_end(
        &mut state,
        &mut events,
        Phase::Enemy,
        EventTiming::After,
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 7, "native effect mutated state");
}

#[test]
fn native_effect_rejects_unknown_tag() {
    let mut state = state_with_agenda(AGENDA_BAD);
    let mut events = Vec::new();
    let outcome = test_support::fire_forced_on_phase_end(
        &mut state,
        &mut events,
        Phase::Enemy,
        EventTiming::After,
    );
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "unknown tag rejects"
    );
    assert_eq!(state.agenda_doom, 0, "no mutation on reject");
}
