//! #509: drawing a persistent treachery weakness (Cover Up 01007) from the
//! player deck during play reveals it and resolves its Revelation — Cover Up
//! enters the controller's threat area with 3 clues instead of staying in hand.

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    dispatch_turn_action_unchecked, test_investigator, test_location, GameStateBuilder,
};
use game_core::TurnAction;

const COVER_UP: &str = "01007";
const HOLY_ROSARY: &str = "01059"; // a non-weakness asset, for the negative case

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Solo investigator at a revealed location, mid-Investigation, 3 actions, no
/// enemies (so the Draw action's `AoO` loop is empty and resolves synchronously),
/// with `deck_top` as the top card of an otherwise-filler deck.
fn draw_state(deck_top: &str) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc = LocationId(101);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.actions_remaining = 3;
    // Top of deck is drawn first (draw_cards drains from the front).
    inv.deck = vec![
        CardCode::new(deck_top),
        CardCode::new(HOLY_ROSARY),
        CardCode::new(HOLY_ROSARY),
    ];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_location(test_location(101, "Study"))
        .build();
    (state, id)
}

#[test]
fn drawing_cover_up_reveals_it_into_the_threat_area() {
    let (state, id) = draw_state(COVER_UP);

    let result = dispatch_turn_action_unchecked(state, &TurnAction::Draw { investigator: id });

    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "Draw must not be rejected; got {:?}",
        result.outcome,
    );
    let inv = &result.state.investigators[&id];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "Cover Up must not stay in hand — it is revealed on draw",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up should be in the threat area after being drawn");
    assert_eq!(
        placed.clues, 3,
        "Cover Up enters the threat area with 3 clues"
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, Event::CardRevealed { code, .. } if code.as_str() == COVER_UP)),
        "a CardRevealed event must fire for the drawn weakness",
    );
}

#[test]
fn drawing_a_non_weakness_leaves_it_in_hand() {
    let (state, id) = draw_state(HOLY_ROSARY);

    let result = dispatch_turn_action_unchecked(state, &TurnAction::Draw { investigator: id });

    let inv = &result.state.investigators[&id];
    assert!(
        inv.hand.iter().any(|c| c.as_str() == HOLY_ROSARY),
        "a normal drawn card stays in hand",
    );
    assert!(inv.threat_area.is_empty(), "nothing enters the threat area");
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, Event::CardRevealed { .. })),
        "no reveal for a non-weakness draw",
    );
}
