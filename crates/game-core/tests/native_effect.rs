//! `Effect::Native` dispatch: a card's `native(tag)` effect resolves
//! through `CardRegistry.native_effect_for` to a host-provided Rust fn.
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)`.

use std::sync::OnceLock;

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{fire_forced_on_phase_end, test_investigator, GameStateBuilder};
use game_core::{Cx, EngineOutcome, EvalContext};

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == AGENDA {
        // Forced at end of enemy phase -> a native effect tagged "test:set-doom".
        Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: card_dsl::dsl::Phase::Enemy,
            },
            EventTiming::After,
            native("test:set-doom"),
        )])
    } else if code.as_str() == AGENDA_BAD {
        Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: card_dsl::dsl::Phase::Enemy,
            },
            EventTiming::After,
            native("test:missing"),
        )])
    } else {
        None
    }
}

fn set_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 7;
    EngineOutcome::Done
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        "test:set-doom" => Some(set_doom),
        _ => None,
    }
}

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: mock_native_for,
            native_eligibility_for: |_| None,
        });
    });
}

fn state_with_agenda(code: &str) -> GameState {
    // `turn_order` must be non-empty: `PhaseEnded` forced dispatch binds
    // the controller to `turn_order.first()` and returns no hits otherwise.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn native_effect_runs_via_registry() {
    install();
    let mut state = state_with_agenda(AGENDA);
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 7, "native effect mutated state");
}

#[test]
fn native_effect_rejects_unknown_tag() {
    install();
    let mut state = state_with_agenda(AGENDA_BAD);
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "unknown tag rejects"
    );
    assert_eq!(state.agenda_doom, 0, "no mutation on reject");
}
