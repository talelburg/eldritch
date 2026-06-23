//! Act round-end window through the public `apply` entry: the action-gate
//! guard blocks non-`ResolveInput` actions while a window is pending, and
//! `ResolveInput` routes to the resume (Confirm spends + advances).

use std::sync::OnceLock;

use card_dsl::dsl::{native, reaction_on_event, Ability, EventPattern, EventTiming};
use game_core::action::{InputResponse, PlayerAction};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::state::{
    Act, ActRoundEndPending, CardCode, GameState, InvestigatorId, Location, LocationId, Phase,
    RoundEndAdvance,
};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::{apply, round_end_advance, Action, Cx, EngineOutcome, EvalContext};

/// The advance logic now lives in the registry (01109's `When`-`RoundEnded`
/// reaction native), so the resume fires it through the effect evaluator. A
/// minimal mock registry stands in for `cards`: act 01109 exposes the `When`
/// advance reaction whose native delegates to the engine's group clue-spend.
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

/// Act 2 current, a Hallway investigator with `clues`, and the round-end
/// window already parked (so we test the guard + routing, not the phase
/// cycle that opens it — that's covered by the `phases.rs` unit tests).
fn parked_window_state(clues: u8) -> GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([inv])
        .with_phase(Phase::Upkeep)
        // UpkeepPhase anchor (slice 1a): the round-end teardown pops it.
        .with_phase_anchor(game_core::state::Continuation::UpkeepPhase {
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
            round_end_advance: Some(RoundEndAdvance {
                contributor_location: CardCode("01112".into()),
            }),
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 0,
            resolution: Some(game_core::scenario::Resolution::Won { id: "R1".into() }),
            round_end_advance: None,
        },
    ];
    state.act_index = 0;
    state
        .continuations
        .push(game_core::state::Continuation::ActRoundEnd(
            ActRoundEndPending {
                contributor_location: LocationId(2),
                threshold: 3,
            },
        ));
    state
}

#[test]
fn pending_window_blocks_non_resolve_actions() {
    let state = parked_window_state(3);
    let r = apply(state, Action::Player(PlayerAction::EndTurn));
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "the guard blocks non-ResolveInput actions while a window is pending"
    );
    assert!(
        matches!(
            r.state.continuations.last(),
            Some(game_core::state::Continuation::ActRoundEnd(_))
        ),
        "still pending"
    );
}

#[test]
fn resolve_confirm_routes_to_resume_and_advances() {
    install();
    let state = parked_window_state(3);
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    // Confirming the round-end window continues the upkeep cascade into Mythos,
    // which pauses at the step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r.state.act_index, 1, "advanced act 2 -> act 3");
    assert_eq!(
        r.state.investigators[&InvestigatorId(1)].clues,
        0,
        "spent 3"
    );
    assert!(!matches!(
        r.state.continuations.last(),
        Some(game_core::state::Continuation::ActRoundEnd(_))
    ));
}
