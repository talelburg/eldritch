//! Amnesia 01096, a basic weakness, from its printed card.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01096):
//! > **Revelation** - Choose and discard all but 1 card from your hand.
//!
//! Ruling (<https://arkhamdb.com/card/01096>):
//! > RRG entry for Weakness says: "A player may not optionally choose to
//! > discard a weakness card from hand". So if you have Dark Memory in hand,
//! > you must keep it and discard all other cards.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Zone};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event_count, assert_event_sequence};

const AMNESIA: &str = "01096";
const DARK_MEMORY: &str = "01013";
const GUTS: &str = "01089";
const LUCKY: &str = "01080";
const HOLY_ROSARY: &str = "01059";
const LEATHER_COAT: &str = "01072";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The bearer at `HERE` holding `hand`, Amnesia on top of their deck, drawing
/// it with the Draw action.
fn draw_holding(hand: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .hand(hand.iter().copied())
                .deck([AMNESIA, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV })
}

fn hand_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: INV,
        hand_index,
    }
}

fn sorted(codes: &[CardCode]) -> Vec<CardCode> {
    let mut codes = codes.to_vec();
    codes.sort();
    codes
}

/// *"Choose and discard all but 1 card from your hand."* With 4 cards in hand
/// the bearer is offered each of them to keep, keeps the one they choose, and
/// discards the other 3; then Amnesia itself goes to the discard pile.
#[test]
fn with_four_cards_it_keeps_the_chosen_one_and_discards_three() {
    let session = draw_holding(&[GUTS, LUCKY, HOLY_ROSARY, LEATHER_COAT]);

    for i in 0..4 {
        assert_eq!(session.offered_at(&hand_card(i)), 1, "hand card {i}");
    }
    let session = session.pick(hand_card(2));

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(HOLY_ROSARY)]);
    assert_eq!(
        sorted(&inv.discard),
        sorted(&[
            CardCode::new(GUTS),
            CardCode::new(LUCKY),
            CardCode::new(LEATHER_COAT),
            CardCode::new(AMNESIA),
        ])
    );
    assert_eq!(inv.discard.last(), Some(&CardCode::new(AMNESIA)));
    assert_event_count!(
        session.events(),
        3,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == AMNESIA,
        Event::CardDiscarded { from: Zone::Hand, .. },
        Event::CardDiscarded {
            code,
            from: Zone::Deck,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == AMNESIA && *to == INV,
    );
}

/// The ruling: with Dark Memory in hand, *"you must keep it and discard all
/// other cards"*. The weakness is the only card that may be kept, so no choice
/// is offered.
#[test]
fn with_dark_memory_in_hand_it_keeps_dark_memory_without_a_choice() {
    let session = draw_holding(&[GUTS, DARK_MEMORY, LUCKY]);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(DARK_MEMORY)]);
    assert_eq!(
        sorted(&inv.discard),
        sorted(&[
            CardCode::new(GUTS),
            CardCode::new(LUCKY),
            CardCode::new(AMNESIA),
        ])
    );
}

/// A lone card in hand is the 1 kept: nothing is discarded but Amnesia.
#[test]
fn with_one_card_in_hand_it_discards_nothing() {
    let session = draw_holding(&[GUTS]);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(GUTS)]);
    assert_eq!(inv.discard, vec![CardCode::new(AMNESIA)]);
    assert_event_count!(
        session.events(),
        0,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
}

/// An empty hand has nothing to keep or discard.
#[test]
fn with_an_empty_hand_it_does_nothing_but_go_to_the_discard_pile() {
    let session = draw_holding(&[]);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(inv.hand.is_empty());
    assert_eq!(inv.discard, vec![CardCode::new(AMNESIA)]);
}
