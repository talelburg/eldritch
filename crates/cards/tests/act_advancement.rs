//! Act-3 objective: defeating the Ghoul Priest (01116) advances Act 3
//! (01110) to its terminal Won resolution. The Ghoul Priest enemy + its
//! spawn land in C3 (#231); here we drive the forced dispatch directly
//! with the real registry. End-to-end defeat->Won via a real Fight is
//! C7b (#245).

use game_core::engine::{EngineOutcome, TurnAction};
use game_core::scenario::Resolution;
use game_core::state::{Act, CardCode, InvestigatorId, Phase};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, GameStateBuilder,
};

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn act3_state() -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new().with_turn_order([inv]).build();
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];
    state
}

#[test]
fn defeating_ghoul_priest_advances_act_3_to_won() {
    let mut state = act3_state();
    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01116".into()), // the Ghoul Priest
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        matches!(state.resolution, Some(Resolution::Won { .. })),
        "Ghoul Priest defeat should set resolution to Won"
    );
}

#[test]
fn defeating_other_enemy_does_not_advance_act_3() {
    let mut state = act3_state();
    let mut events = Vec::new();
    let out = game_core::test_support::fire_forced_on_enemy_defeat(
        &mut state,
        &mut events,
        CardCode("01103".into()), // some other enemy, not the Ghoul Priest
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        state.resolution.is_none(),
        "only the Ghoul Priest's defeat advances Act 3"
    );
}

/// Act 01110 ("What Have You Done?") advances only via its Forced
/// `EnemyDefeated` objective (the Ghoul Priest), so its corpus clue threshold is
/// `null` -> 0. The deliberate clue-spend `AdvanceAct` action must be rejected
/// for it — otherwise the player could "spend 0 clues to advance" and instantly
/// latch the terminal Won resolution, bypassing the Ghoul Priest fight (#486).
/// The legitimate defeat path stays covered by
/// `defeating_ghoul_priest_advances_act_3_to_won` above.
#[test]
fn advance_act_action_rejected_for_act_3_objective() {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 5; // plenty — reject must be the objective, not affordability
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    // Act 3 is current and terminal-Won (mirrors the_gathering setup()).
    state.act_deck = vec![Act {
        code: CardCode("01110".into()),
        clue_threshold: 0,
        resolution: Some(Resolution::Won { id: "R1".into() }),
    }];

    let result =
        dispatch_turn_action_unchecked(state, &TurnAction::AdvanceAct { investigator: inv });
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "AdvanceAct must be rejected for Act 3's non-clue objective"
    );
    assert!(
        result.state.resolution.is_none(),
        "rejected AdvanceAct must not latch the Won resolution (no instant win)"
    );
    assert_eq!(result.state.act_index, 0, "act did not advance");
    assert_eq!(result.state.investigators[&inv].clues, 5, "no clues spent");
}

/// Act 01109 ("The Barrier") advances only at the end of the round (its
/// `When`-`RoundEnded` group objective), so the `AdvanceAct` *action* is rejected.
/// Registry-based detection (`act_advances_at_round_end`, #434) — needs the real
/// registry, so it lives here rather than as a game-core lib unit test.
#[test]
fn advance_act_rejected_for_round_end_advance_act() {
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.clues = 9; // plenty — reject must be the objective, not affordability
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(investigator)
        .with_active_investigator(inv)
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act {
        code: CardCode("01109".into()),
        clue_threshold: 3,
        resolution: None,
    }];

    let result =
        dispatch_turn_action_unchecked(state, &TurnAction::AdvanceAct { investigator: inv });
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.act_index, 0, "act did not advance");
    assert_eq!(result.state.investigators[&inv].clues, 9, "no clues spent");
}
