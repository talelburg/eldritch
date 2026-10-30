//! Seating a roster resolves investigator stats from the real corpus
//! ([`game_core::card_registry::CardRegistry`]) and takes the deck from the
//! payload.
//!
//! Integration test so it can run against `cards::TEST_REGISTRIES` (per the test layering in
//! `docs/agents/standards.md`). The playable-gate tests run against the
//! production `cards::REGISTRY` instead, since `TEST_REGISTRIES` accepts every
//! code.

use cards::{REGISTRIES, TEST_REGISTRIES};
use game_core::action::RosterEntry;
use game_core::engine::{self, EngineOutcome};
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, Skills};

#[test]
fn seats_roland_with_corpus_stats_and_payload_deck() {
    let deck = vec![CardCode::new("01030"), CardCode::new("01030")];
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: deck.clone(),
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .build();

    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);

    // seat_and_open seats the roster and opens the setup mulligan prompt
    // (AwaitingInput) for the first investigator.
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
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
    assert_eq!(inv.max_health(TEST_REGISTRIES.cards), 9);
    assert_eq!(inv.max_sanity(TEST_REGISTRIES.cards), 5);
    // Deck + hand together account for the 2 supplied cards (the 5-card
    // opening-hand draw takes only what's available).
    assert_eq!(inv.deck.len() + inv.hand.len(), deck.len());
}

/// Agnes Baker 01004 (`pack/core/core.json`): Willpower 5, Intellect 2,
/// Combat 2, Agility 3; health 6, sanity 8.
#[test]
fn seats_agnes_with_corpus_stats_and_payload_deck() {
    let deck = vec![CardCode::new("01059"), CardCode::new("01088")];
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01004"),
        deck: deck.clone(),
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .build();

    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
    let inv = result
        .state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Agnes seated at id 1");
    assert_eq!(inv.name, "Agnes Baker");
    assert_eq!(inv.investigator_card.code, CardCode::new("01004"));
    assert_eq!(
        inv.skills,
        Skills {
            willpower: 5,
            intellect: 2,
            combat: 2,
            agility: 3
        }
    );
    assert_eq!(inv.max_health(TEST_REGISTRIES.cards), 6);
    assert_eq!(inv.max_sanity(TEST_REGISTRIES.cards), 8);
    assert_eq!(inv.deck.len() + inv.hand.len(), deck.len());
}

#[test]
fn rejects_non_investigator_code() {
    // 01030 (Magnifying Glass) is an Asset, not an investigator.
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01030"),
        deck: vec![],
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .build();
    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}

#[test]
fn seated_investigator_carries_its_card_code() {
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .build();
    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);
    let inv = result
        .state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated at id 1");
    assert_eq!(inv.investigator_card.code, CardCode::new("01001"));
}

/// Shotgun 01029 and Hot Streak 01057: core-set cards with no implementation.
const SHOTGUN: &str = "01029";
const HOT_STREAK: &str = "01057";

/// Seat Roland 01001 with `deck` under [`REGISTRIES`], asserting the
/// seating is refused with state unchanged, and return the reason.
fn refused_reason(deck: &[&str]) -> String {
    for code in [SHOTGUN, HOT_STREAK] {
        assert!(
            !cards::is_playable(code),
            "{code} is implemented now; pick another unimplemented card"
        );
    }
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: deck.iter().map(|c| CardCode::new(*c)).collect(),
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new().with_registries(REGISTRIES).build();
    let result = engine::seat_and_open(state.clone(), &roster, 0, REGISTRIES);
    let EngineOutcome::Rejected { reason } = result.outcome else {
        panic!(
            "a deck with unimplemented cards is refused, got {:?}",
            result.outcome
        );
    };
    assert_eq!(
        result.state, state,
        "a refused seating leaves state unchanged"
    );
    assert!(result.events.is_empty());
    reason.to_string()
}

#[test]
fn refuses_a_deck_with_an_unimplemented_card_and_names_it() {
    let reason = refused_reason(&["01030", SHOTGUN]);
    assert!(reason.contains(SHOTGUN), "reason names {SHOTGUN}: {reason}");
}

#[test]
fn refuses_a_deck_with_two_unimplemented_cards_and_names_both() {
    let reason = refused_reason(&[SHOTGUN, "01030", HOT_STREAK, SHOTGUN]);
    assert!(reason.contains(SHOTGUN), "reason names {SHOTGUN}: {reason}");
    assert!(
        reason.contains(HOT_STREAK),
        "reason names {HOT_STREAK}: {reason}"
    );
    assert_eq!(
        reason.matches(SHOTGUN).count(),
        1,
        "a repeated code is named once: {reason}"
    );
}

#[test]
fn rejects_unknown_code() {
    let roster = vec![RosterEntry {
        investigator: CardCode::new("99999"),
        deck: vec![],
        random_basic_weaknesses: 0,
    }];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .build();
    let result = engine::seat_and_open(state, &roster, 0, TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 0);
    assert!(result.events.is_empty());
}
