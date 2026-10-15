//! End-to-end tests for Leather Coat (01072), a Body asset with 2 health and
//! no text.
//!
//! Everything the card does is printed metadata
//! (`data/arkhamdb-snapshot/pack/core/core.json`: `"slot": "Body"`,
//! `"health": 2`, no `text`), and it has no rulings
//! (`data/arkhamdb-faq/no-rulings.txt`). So these tests check that the
//! metadata reaches the engine: the coat takes the Body slot, soaks damage
//! assigned to its controller until its 2 health is spent, and soaks no
//! horror.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Zone};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const LEATHER_COAT: &str = "01072";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Investigator 1 at an open turn in the Study, with `hand` in hand and
/// Leather Coat in play when `coat_in_play`.
fn board(hand: &[&str], coat_in_play: bool) -> TestSession {
    let seat = seat(1).at(HERE).resources(5).hand(hand.iter().copied());
    let seat = if coat_in_play {
        seat.in_play([LEATHER_COAT])
    } else {
        seat
    };
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat)
        .open_turn(INV)
        .session()
}

/// Play the first card in hand from the turn menu.
fn play_first(session: TestSession) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

/// The seated coat's anchor, for soaking onto it.
fn coat(session: &TestSession) -> OptionTarget {
    session.state().card(INV, LEATHER_COAT)
}

#[test]
fn a_second_leather_coat_displaces_the_first_from_the_body_slot() {
    let session = play_first(board(&[LEATHER_COAT], true));

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.cards_in_play.len(), 1, "one Body slot holds one coat");
    assert_eq!(inv.discard, vec![CardCode::new(LEATHER_COAT)]);
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::InPlay, to: DiscardPile::Investigator(i) }
            if *i == INV && code.as_str() == LEATHER_COAT
    );
}

#[test]
fn leather_coat_soaks_damage_until_its_two_health_is_spent() {
    let session = board(&[], true);
    let coat = coat(&session);

    let session = session.take_damage(INV, 3).pick(coat.clone()).pick(coat);

    // Two points on the coat defeat it; the third has nowhere else to go.
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 1, "the coat took two of the three");
    assert!(inv.cards_in_play.is_empty(), "the spent coat is discarded");
    assert_eq!(inv.discard, vec![CardCode::new(LEATHER_COAT)]);
}

#[test]
fn leather_coat_soaks_no_horror() {
    let session = board(&[], true);
    let coat = coat(&session);

    let session = session.take_horror(INV, 1);

    assert_eq!(session.offered_at(&coat), 0, "the coat has no sanity");
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.cards_in_play.len(), 1, "the coat is untouched");
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}
