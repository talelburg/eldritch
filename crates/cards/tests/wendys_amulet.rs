//! Wendy's Amulet 01014's Forced, end to end against the real card registry
//! (#1109). Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Wendy Adams deck only.
//! > You may play the topmost event in your discard pile as if it were in your hand.
//! > <b>Forced</b> - After you play an event or discard an event from play: Place it on the bottom of your deck instead of in your discard pile.
//!
//! Rulings (<https://arkhamdb.com/card/01014>):
//!
//! > If your deck is empty, and Wendy’s Amulet would have you place an event
//! > “on the bottom of your deck,” that event becomes your new deck.
//!
//! The play permission is #1112's; these tests cover the redirect only, at
//! both of its sites: a played event's step-4 placement, and an event leaving
//! play (Barricade 01038, which stays in play attached to a location).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{Agenda, CardCode, GameStateBuilder, InvestigatorId, Location, LocationId};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const AMULET: &str = "01014";
/// Event. `Supply.` Gain 3 resources.
const EMERGENCY_CACHE: &str = "01088";
/// Event. `Spell.` Places 1 doom on the current agenda.
const DARK_MEMORY: &str = "01013";
const HEIRLOOM: &str = "01012";
/// Event that stays in play, attached to the location.
const BARRICADE: &str = "01038";
/// Deck filler.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

const PLAY_FIRST: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// `investigator`'s open turn at `HERE`, solo, with The Gathering's first
/// agenda current (Dark Memory places its doom there).
fn board(investigator: Seat) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    TestSession::new(state, TEST_REGISTRIES)
}

fn codes(codes: &[&str]) -> Vec<CardCode> {
    codes.iter().copied().map(CardCode::new).collect()
}

#[test]
fn a_played_event_goes_to_the_bottom_of_the_deck() {
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET])
            .hand([EMERGENCY_CACHE])
            .deck([GUTS]),
    )
    .take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(
        inv.deck,
        codes(&[GUTS, EMERGENCY_CACHE]),
        "placed on the bottom"
    );
    assert!(inv.discard.is_empty(), "instead of in the discard pile");
    assert_event!(
        s.events(),
        Event::CardPlacedOnBottomOfDeck { investigator: INV, code } if code == &CardCode::new(EMERGENCY_CACHE)
    );
    assert_no_event!(s.events(), Event::CardDiscarded { .. });
    assert_event!(s.events(), Event::CardPlayed { .. });
}

#[test]
fn into_an_empty_deck_the_event_becomes_the_deck() {
    let s = board(seat(1).at(HERE).in_play([AMULET]).hand([EMERGENCY_CACHE])).take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[EMERGENCY_CACHE]));
    assert!(inv.discard.is_empty());
}

#[test]
fn without_the_amulet_a_played_event_is_discarded() {
    let s = board(seat(1).at(HERE).hand([EMERGENCY_CACHE]).deck([GUTS])).take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[GUTS]));
    assert_eq!(inv.discard, codes(&[EMERGENCY_CACHE]));
    assert_no_event!(s.events(), Event::CardPlacedOnBottomOfDeck { .. });
}

/// *"After **you** play"*, and *"**your** deck"*: another investigator's
/// Amulet does not reach investigator 1's event.
#[test]
fn another_investigators_amulet_does_not_redirect_your_event() {
    let s = TestSession::new(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_seat(seat(1).at(HERE).hand([EMERGENCY_CACHE]))
            .with_seat(seat(2).at(HERE).in_play([AMULET]))
            .with_turn_order([INV, INV2])
            .open_turn(INV)
            .build(),
        TEST_REGISTRIES,
    )
    .take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].discard,
        codes(&[EMERGENCY_CACHE])
    );
    assert!(s.state().investigators[&INV2].deck.is_empty());
}

/// Official FAQ: *"An effect that triggers “after you play” an event but does
/// not modify where the event goes (such as Heirloom of Hyberborea’s ability)
/// would naturally trigger just after step 4."* — so Heirloom's window opens
/// with Dark Memory already on the bottom of the deck, and its draw takes the
/// top card.
#[test]
fn an_after_you_play_reaction_still_fires_after_placement() {
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET, HEIRLOOM])
            .hand([DARK_MEMORY])
            .resources(5)
            .deck([GUTS]),
    )
    .take(&PLAY_FIRST);

    let heirloom = s.state().card(INV, HEIRLOOM);
    assert_eq!(s.offered_at(&heirloom), 1, "Heirloom is offered");
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes(&[GUTS, DARK_MEMORY]),
        "Dark Memory is already on the bottom when the window opens",
    );

    let s = s.pick(heirloom);
    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.hand, codes(&[GUTS]), "Heirloom drew the top card");
    assert_eq!(inv.deck, codes(&[DARK_MEMORY]));
}

/// The from-play arm with a printed card: Barricade stays in play attached to
/// the location, and its own Forced discards it when an investigator leaves.
/// Barricade is a Seeker card Wendy cannot take; a test board is not
/// deckbuilding-validated.
#[test]
fn an_event_discarded_from_play_goes_to_the_bottom_of_the_deck() {
    let mut here = test_support::test_location(HERE.0, "Study");
    here.connections = vec![THERE];
    let mut there: Location = test_support::test_location(THERE.0, "Hallway");
    there.connections = vec![HERE];
    let s = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(here)
        .with_location(there)
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([AMULET])
                .hand([BARRICADE])
                .deck([GUTS]),
        )
        .open_turn(INV)
        .session()
        .take(&PLAY_FIRST);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes(&[GUTS]),
        "an attached event is not placed by its play",
    );

    let s = s.take(&TurnAction::Move {
        investigator: INV,
        destination: THERE,
    });

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[GUTS, BARRICADE]));
    assert!(inv.discard.is_empty());
    assert!(s.state().locations[&HERE].attachments.is_empty());
    assert_event!(
        s.events(),
        Event::CardPlacedOnBottomOfDeck { investigator: INV, code } if code == &CardCode::new(BARRICADE)
    );
    assert_no_event!(s.events(), Event::CardDiscarded { .. });
}
