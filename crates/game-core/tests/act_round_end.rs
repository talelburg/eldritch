//! Act round-end window through the public `apply` entry: the action-gate
//! guard blocks non-`ResolveInput` actions while a window is pending, and
//! `ResolveInput` routes to the resume (Confirm spends + advances).

use game_core::action::{InputResponse, PlayerAction};
use game_core::state::{
    Act, ActRoundEndPending, CardCode, GameState, InvestigatorId, Location, LocationId, Phase,
    RoundEndAdvance,
};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::{apply, Action, EngineOutcome};

/// Act 2 current, a Hallway investigator with `clues`, and the round-end
/// window already parked (so we test the guard + routing, not the phase
/// cycle that opens it — that's covered by the `phases.rs` unit tests).
fn parked_window_state(clues: u8) -> GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([inv])
        .with_phase(Phase::Upkeep)
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
    state.act_round_end_pending = Some(ActRoundEndPending {
        contributor_location: LocationId(2),
        threshold: 3,
    });
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
    assert!(r.state.act_round_end_pending.is_some(), "still pending");
}

#[test]
fn resolve_confirm_routes_to_resume_and_advances() {
    let state = parked_window_state(3);
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_eq!(r.state.act_index, 1, "advanced act 2 -> act 3");
    assert_eq!(r.state.investigators[&InvestigatorId(1)].clues, 0, "spent 3");
    assert!(r.state.act_round_end_pending.is_none());
}
