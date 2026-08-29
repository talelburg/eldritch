//! `Condition::Native` dispatch (#592): a card's `native_condition(tag)`
//! predicate resolves through `CardRegistry.native_condition_for` to a
//! host-provided `fn(&GameState, &EvalContext) -> bool`, and gates the
//! surrounding effect on its verdict.
//!
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)` — same shape as `native_effect.rs`.

use card_dsl::dsl::{
    forced_on_event, gain_resources, if_else, native_condition, Ability, EventPattern, EventTiming,
    InvestigatorTarget,
};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeConditionFn};
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{fire_forced_on_phase_end, test_investigator, GameStateBuilder};
use game_core::{EngineOutcome, EvalContext};

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";
const INV: InvestigatorId = InvestigatorId(1);

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

/// Forced at end of the enemy phase: gain 2 resources when the native
/// predicate holds, 5 when it does not — two distinct observable branches.
fn gated(tag: &str) -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::PhaseEnded {
            phase: card_dsl::dsl::Phase::Enemy,
        },
        EventTiming::After,
        if_else(
            native_condition(tag),
            gain_resources(InvestigatorTarget::You, 2),
            gain_resources(InvestigatorTarget::You, 5),
        ),
    )]
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        AGENDA => Some(gated("test:has-doom")),
        AGENDA_BAD => Some(gated("test:missing")),
        _ => None,
    }
}

/// Reads both arguments the predicate signature provides: board state and the
/// evaluation context's controller.
fn has_doom(state: &GameState, ctx: &EvalContext) -> bool {
    state.agenda_doom > 0 && state.investigators.contains_key(&ctx.controller)
}

fn mock_native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    match tag {
        "test:has-doom" => Some(has_doom as NativeConditionFn),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
        back_abilities_for: |_| None,
        native_condition_for: mock_native_condition_for,
    });
}

fn state_with_agenda(code: &str, doom: u8) -> GameState {
    // `turn_order` must be non-empty: `PhaseEnded` forced dispatch binds
    // the controller to `turn_order.first()` and returns no hits otherwise.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
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
    let mut state = state_with_agenda(AGENDA, 1);
    let before = resources(&state);
    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(resources(&state), before + 2, "predicate true → `then`");
}

#[test]
fn native_condition_failing_takes_the_else_branch() {
    let mut state = state_with_agenda(AGENDA, 0);
    let before = resources(&state);
    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(resources(&state), before + 5, "predicate false → `else_`");
}

/// An unregistered tag is a card-authoring bug, not a `false` verdict — it
/// rejects loudly rather than silently taking the `else_` branch.
#[test]
fn native_condition_rejects_unknown_tag() {
    let mut state = state_with_agenda(AGENDA_BAD, 1);
    let before = resources(&state);
    let mut events = Vec::new();
    let outcome =
        fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::After);
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "unknown tag rejects; got {outcome:?}"
    );
    assert_eq!(resources(&state), before, "no mutation on reject");
}
