//! Act-3 objective: defeating the Ghoul Priest (01116) advances Act 3
//! (01110) to its terminal Won resolution. The Ghoul Priest enemy + its
//! spawn land in C3 (#231); here we drive the forced dispatch directly
//! with the real registry. End-to-end defeat->Won via a real Fight is
//! C7b (#245).

use game_core::engine::EngineOutcome;
use game_core::scenario::Resolution;
use game_core::state::{Act, CardCode, InvestigatorId};
use game_core::test_support::GameStateBuilder;

fn act3_state() -> game_core::state::GameState {
    let _ = game_core::card_registry::install(cards::REGISTRY);
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
