//! #482 act-path proof: an *interactive* act on-advance reverse (a synthetic
//! Forced `ChooseOne`) resolves cleanly through the `AdvanceReverse` frame — it
//! does not strand, mirroring the agenda path. No such card exists in the corpus,
//! so we install a mock registry that gives act code `_iact` a `ChooseOne` reverse.

use card_dsl::dsl::{
    choose_one, deal_horror, forced_on_event, Ability, EventPattern, EventTiming,
    InvestigatorTarget,
};
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::engine::EngineOutcome;
use game_core::state::{Act, CardCode, InvestigatorId};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, GameStateBuilder,
};
use game_core::{InputKind, TurnAction};

const IACT: &str = "_iact";

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == IACT).then(|| {
        vec![forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            // Two always-legal branches ⇒ the choice suspends.
            choose_one(vec![
                deal_horror(InvestigatorTarget::You, 1u8),
                deal_horror(InvestigatorTarget::You, 2u8),
            ]),
        )]
    })
}

fn metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(CardRegistry {
        metadata_for,
        abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
    });
}

#[test]
fn interactive_act_reverse_resolves_cleanly() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_phase(game_core::state::Phase::Investigation)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .with_investigator(test_investigator(1))
        .build();
    // Two acts: the leaving one (_iact, threshold 0 so AdvanceAct is affordable)
    // carries the interactive reverse; a successor so the advance is non-terminal.
    state.act_deck = vec![
        Act {
            code: CardCode(IACT.into()),
            clue_threshold: 0,
            resolution: None,
        },
        Act {
            code: CardCode("_iact_2".into()),
            clue_threshold: 3,
            resolution: None,
        },
    ];
    state.act_index = 0;

    // dispatch_turn_action_unchecked bypasses the open-turn enumeration gate (the
    // synthetic state has no InvestigatorTurn frame); the handler still validates.
    let r = dispatch_turn_action_unchecked(state, &TurnAction::AdvanceAct { investigator: inv });

    // The act's interactive reverse is the live prompt (it did not strand); the
    // act cursor has NOT bumped yet (Finalize runs after the choice).
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("expected the act reverse ChooseOne, got {:?}", r.outcome);
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert_eq!(
        r.state.act_index, 0,
        "cursor bumps only after the reverse resolves"
    );
}
