//! `ForcedTriggerPoint::RoundEnded`: an agenda's `OnEvent(RoundEnded)`
//! Forced ability fires at the end of the round (step 4.6).

use std::sync::OnceLock;

use card_dsl::dsl::{native, on_event, Ability, EventPattern, EventTiming};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::state::{Agenda, CardCode, InvestigatorId};
use game_core::test_support::{fire_forced_on_round_end, test_investigator, GameStateBuilder};
use game_core::{Cx, EngineOutcome, EvalContext};

const AGENDA: &str = "TEST-AGENDA";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == AGENDA).then(|| {
        vec![on_event(
            EventPattern::RoundEnded,
            EventTiming::After,
            native("test:set-doom"),
        )]
    })
}

fn set_doom(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    cx.state.agenda_doom = 5;
    EngineOutcome::Done
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == "test:set-doom").then_some(set_doom as NativeEffectFn)
}

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: mock_native_for,
        });
    });
}

#[test]
fn round_ended_fires_agenda_forced_ability() {
    install();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 5, "RoundEnded fired the agenda ability");
}
