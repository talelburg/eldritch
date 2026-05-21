//! Integration tests for the Fast play-card gate loosening from #103.
//!
//! Verifies that a Fast card (Magnifying Glass, 01030) can be played
//! by a non-active investigator when an open window's `fast_actors`
//! scope permits, and that a non-Fast asset (Holy Rosary, 01059) in
//! the same setup is still rejected.

use std::sync::Once;

use game_core::action::{Action, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, FastActorScope, InvestigatorId, Phase, WindowKind};
use game_core::test_support::{test_investigator, TestGame};

fn install_cards_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

#[test]
fn fast_asset_playable_by_non_active_investigator_when_window_permits() {
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Magnifying Glass should play Fast from non-active investigator's hand: {:?}",
        result.outcome,
    );
    let b_after = result.state.investigators.get(&InvestigatorId(2)).unwrap();
    assert_eq!(b_after.hand.len(), 0, "card should have left hand");
    assert_eq!(b_after.cards_in_play.len(), 1, "card should be in play");
}

#[test]
fn non_fast_asset_still_rejected_when_not_active_investigator() {
    install_cards_registry();
    let a = test_investigator(1);
    let mut b = test_investigator(2);
    b.resources = 5;
    b.hand.push(CardCode::new("01059")); // Holy Rosary — non-Fast asset, cost 2.
    let state = TestGame::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .with_open_window(
            WindowKind::BetweenPhases {
                from: Phase::Mythos,
                to: Phase::Investigation,
            },
            FastActorScope::Any,
        )
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(2),
            hand_index: 0,
        }),
    );
    let reason = match result.outcome {
        EngineOutcome::Rejected { reason } => reason,
        other => {
            panic!("Holy Rosary is not Fast — non-active investigator must not play it: {other:?}")
        }
    };
    // Make sure the rejection cites the Fast/active-investigator gate,
    // not (for instance) the missing-from-hand or resource-shortage paths.
    assert!(
        reason.contains("non-Fast")
            || reason.contains("Investigation")
            || reason.contains("active"),
        "expected non-Fast gate rejection; got: {reason}",
    );
}

#[test]
fn fast_asset_still_playable_by_active_investigator_during_investigation() {
    install_cards_registry();
    let mut a = test_investigator(1);
    a.resources = 5;
    a.hand.push(CardCode::new("01030")); // Magnifying Glass — Fast.
    let state = TestGame::new()
        .with_investigator(a)
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    let result = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: InvestigatorId(1),
            hand_index: 0,
        }),
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "Magnifying Glass plays normally for active investigator (Phase-3 behavior preserved): {:?}",
        result.outcome,
    );
}
