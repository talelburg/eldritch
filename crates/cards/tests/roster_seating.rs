//! B2: seating a roster resolves investigator stats from the real corpus
//! ([`game_core::CardRegistry`]) and takes the deck from the payload. Integration test so
//! it can install `cards::REGISTRY` in its own process (per CLAUDE.md test
//! layering).

use std::sync::Once;

use game_core::action::{PlayerAction, RosterEntry};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, InvestigatorId, Skills};
use game_core::test_support::TestGame;
use game_core::Action;

/// Install the real card registry exactly once for this integration-test
/// binary. Idempotent at the `OnceLock` level; the `Once` wrapper avoids
/// the futile second `install` call.
fn install_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

#[test]
fn seats_roland_with_corpus_stats_and_payload_deck() {
    install_registry();
    let deck = vec![CardCode::new("01030"), CardCode::new("01030")];
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: deck.clone(),
    }];
    let state = TestGame::new().build();

    let result = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = result
        .state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated at id 1");
    assert_eq!(inv.name, "Roland Banks");
    assert_eq!(
        inv.skills,
        Skills {
            willpower: 3,
            intellect: 3,
            combat: 4,
            agility: 2
        }
    );
    assert_eq!(inv.max_health, 9);
    assert_eq!(inv.max_sanity, 5);
    // Deck + hand together account for the 2 supplied cards (the 5-card
    // opening-hand draw takes only what's available).
    assert_eq!(inv.deck.len() + inv.hand.len(), deck.len());
}

#[test]
fn rejects_non_investigator_code() {
    install_registry();
    // 01030 (Magnifying Glass) is an Asset, not an investigator.
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01030"),
        deck: vec![],
    }];
    let state = TestGame::new().build();
    let result = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}

#[test]
fn rejects_unknown_code() {
    install_registry();
    let roster = vec![RosterEntry {
        investigator: CardCode::new("99999"),
        deck: vec![],
    }];
    let state = TestGame::new().build();
    let result = apply(
        state,
        Action::Player(PlayerAction::StartScenario { roster }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}
