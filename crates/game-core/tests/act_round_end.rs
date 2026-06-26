//! Round-end `when` act-advance window through the public `apply` entry
//! (EmitEvent-frame C-coordinators, #434). The Upkeep round-end coordinator
//! opens act 01109's `When`-`RoundEnded` reaction as a board candidate;
//! `ResolveInput(PickSingle)` fires the advance / `Skip` declines.

use card_dsl::dsl::{native, reaction_on_event, Ability, EventPattern, EventTiming};
use game_core::action::{InputResponse, PlayerAction};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::engine::{OptionId, TimingEvent};
use game_core::state::{
    Act, CardCode, Continuation, GameState, InvestigatorId, Location, LocationId, Phase, TimingMode,
};
use game_core::test_support::{run_upkeep_round_end, test_investigator, GameStateBuilder};
use game_core::{apply, round_end_advance, Action, Cx, EngineOutcome, EvalContext};

/// The advance logic lives in the registry (01109's `When`-`RoundEnded` reaction
/// native), so the coordinator fires it through the effect evaluator when its
/// candidate is picked. A minimal mock registry stands in for `cards`.
fn advance_native(cx: &mut Cx, _ctx: &EvalContext) -> EngineOutcome {
    round_end_advance(cx, "01112") // the Hallway
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    (code.as_str() == "01109").then(|| {
        vec![reaction_on_event(
            EventPattern::RoundEnded,
            EventTiming::When,
            native("test:advance"),
        )]
    })
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == "test:advance").then_some(advance_native as NativeEffectFn)
}

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

#[ctor::ctor]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: mock_native_for,
        native_eligibility_for: |_| None,
    });
}

/// Act 2 (01109) current, a Hallway investigator with `clues`, phase Upkeep with
/// its anchor. Act 3 (01110) is the terminal-Won successor.
fn upkeep_round_end_state(clues: u8) -> GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([inv])
        .with_phase(Phase::Upkeep)
        // UpkeepPhase anchor (slice 1a): the round-end teardown pops it.
        .with_phase_anchor(Continuation::UpkeepPhase {
            resume: game_core::state::UpkeepResume::Begins,
        })
        .with_location(Location::new(
            LocationId(2),
            CardCode("01112".into()),
            "Hallway",
            1,
            0,
        ))
        .build();
    let i = state.investigators.get_mut(&inv).unwrap();
    i.current_location = Some(LocationId(2));
    i.clues = clues;
    state.act_deck = vec![
        Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
            resolution: None,
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 0,
            resolution: Some(game_core::scenario::Resolution::Won { id: "R1".into() }),
        },
    ];
    state.act_index = 0;
    state
}

/// Open the round-end `when` window: drive the Upkeep round-end coordinator,
/// which scans act 01109's `When`-`RoundEnded` reaction and suspends on it.
fn opened_round_end_window(clues: u8) -> GameState {
    let mut state = upkeep_round_end_state(clues);
    let mut events = Vec::new();
    let out = run_upkeep_round_end(&mut state, &mut events);
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "the round-end `when` act-advance window should open: {out:?}"
    );
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::TimingPointWindow {
                event: TimingEvent::RoundEnded,
                mode: TimingMode::Reaction,
                ..
            })
        ),
        "the open window is the round-end reaction window, got {:?}",
        state.continuations.last(),
    );
    state
}

#[test]
fn resolve_pick_fires_advance() {
    let state = opened_round_end_window(3);
    // The act-advance is the window's sole candidate (OptionId(0)).
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    // Picking the advance continues the round-end + upkeep cascade into Mythos,
    // which pauses at the step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.act_index, 1, "advanced act 2 -> act 3");
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].clues,
        0,
        "spent 3"
    );
}

#[test]
fn resolve_skip_declines_advance() {
    let state = opened_round_end_window(3);
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.act_index, 0, "no advance on Skip");
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].clues,
        3,
        "no clues spent on Skip"
    );
}
