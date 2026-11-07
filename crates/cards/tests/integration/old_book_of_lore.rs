//! Old Book of Lore 01031's `[action] Exhaust Old Book of
//! Lore: Choose an investigator at your location. That investigator searches
//! the top 3 cards of his or her deck for a card, draws it, and shuffles the
//! remaining cards into his or her deck.` end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Solo: the "choose an investigator at your location" target auto-binds (one
//! co-located investigator), so the only suspend is the top-3 card pick. The
//! exhaust cost is paid before the effect runs. With Roland beside the reader,
//! the investigator is chosen first, and the searcher answers the card pick.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget, Responder};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId,
    LocationId,
};
use game_core::test_support::{location, seat, BoardLookup as _, Seat, TestSession};

const OLD_BOOK: &str = "01031";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
/// A second investigator at `HERE`, for a search of someone else's deck.
const ROLAND: InvestigatorId = InvestigatorId(2);
const BEAT_COP: &str = "01018";
const MAGNIFYING_GLASS: &str = "01030";
const EMERGENCY_CACHE: &str = "01088";

/// Investigator 1 at `HERE` with Old Book of Lore in play and a known 4-card
/// deck (top 3 distinct, plus a 4th below the searched region).
fn reader() -> Seat {
    seat(1)
        .at(HERE)
        .in_play([OLD_BOOK])
        .deck(["90001", "90002", "90003", "90004"])
}

/// `investigator`'s turn, alone at `HERE`.
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// `reader()`'s turn at `HERE`, with Roland beside her holding a known deck:
/// Beat Cop, Magnifying Glass and Emergency Cache on top, then a 4th card.
fn reader_beside_roland() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(reader())
        .with_seat(
            seat(2)
                .at(HERE)
                .deck([BEAT_COP, MAGNIFYING_GLASS, EMERGENCY_CACHE, "90004"]),
        )
        .with_seating([INV, ROLAND])
        .open_turn(INV)
        .session()
}

fn book(session: &TestSession) -> CardInstanceId {
    session.state().in_play(INV, OLD_BOOK)
}

fn activate_book(book: CardInstanceId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(book),
        address: AbilityAddress::Printed(0),
    }
}

/// Take the `position`th card of the searched top 3.
///
/// Exempt from the anchor-pick rule, and sent raw: a deck search's options
/// are cards **in a deck**, with no board anchor and neither of ADR 0015's
/// natures, which that ADR knowingly excludes and defers. No session step can
/// name one, so the pick is positional.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        investigator: INV,
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

#[test]
fn action_searches_top_three_into_hand_then_shuffles() {
    // Activate: exhaust paid, target auto-binds (solo), top 3 give 3 eligible
    // ⇒ the card pick suspends.
    let session = board(reader());
    let book = book(&session);
    let s = session.take(&activate_book(book));
    assert_eq!(
        s.prompt().options.len(),
        3,
        "top 3 ⇒ a card choice suspends: {:?}",
        s.prompt(),
    );
    assert!(
        s.state().investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == book && c.exhausted),
        "exhaust cost paid before the effect",
    );

    // Pick option 1 → the second card of the top 3 ("90002").
    let s = s.apply(take_searched(1));
    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert!(
        inv.hand.contains(&CardCode::new("90002")),
        "picked card moved to hand",
    );
    assert!(
        !inv.deck.contains(&CardCode::new("90002")),
        "picked card removed from deck",
    );
    assert_eq!(inv.deck.len(), 3, "one card left the deck");
    assert_event!(s.events(), Event::CardSearchedToHand { .. });
    assert_event!(s.events(), Event::DeckShuffled { .. });
}

/// Regression for #639: an activation whose effect can't change the game state
/// is rejected. RR "Ability": *"A triggered ability can only be initiated if its
/// effect has the potential to change the game state…"* With an empty deck
/// there is nothing to search, nothing to draw, and nothing to shuffle, so the
/// activation is rejected: the book stays ready and the action is not spent.
#[test]
fn an_empty_deck_cannot_be_searched() {
    let session = board(seat(1).at(HERE).in_play([OLD_BOOK]));
    let book = book(&session);
    let actions_before = session.state().investigators[&INV].actions_remaining;

    // The turn menu already filters this out (`legal_actions` runs the same
    // validator); submit it off the menu to prove the handler *itself* rejects
    // a directly-submitted activation.
    assert_eq!(
        session.offered_at(&OptionTarget::CardInstance(book)),
        0,
        "the turn menu does not offer an activation the validator would reject",
    );
    let r = session.submit_off_menu(&activate_book(book));
    let reason = r.expect_rejected();
    assert!(
        !reason.is_empty(),
        "an empty deck ⇒ the search cannot change the game state: {reason}",
    );
    let inv = &r.state().investigators[&INV];
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.instance_id == book && !c.exhausted),
        "no exhaust paid on a reject",
    );
    assert_eq!(
        inv.actions_remaining, actions_before,
        "no action spent on a reject",
    );
}

/// Old Book of Lore *draws* the card it finds, so a weakness found resolves as
/// a drawn one would. `glossary/Weakness.md`: *"If a weakness enters an
/// investigator's hand in a manner that did not involve drawing the card, that
/// investigator must resolve the card (including any Revelation abilities) as
/// if he or she had just drawn it."* Cover Up 01007, a persistent treachery
/// weakness, goes to the threat area with its 3 clues instead of staying in
/// hand.
#[test]
fn a_weakness_found_resolves_as_if_drawn() {
    const COVER_UP: &str = "01007";
    let session = board(
        seat(1)
            .at(HERE)
            .in_play([OLD_BOOK])
            .deck([COVER_UP, "90002", "90003", "90004"]),
    );
    let book = book(&session);
    let s = session.take(&activate_book(book)).apply(take_searched(0));

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert!(
        !inv.hand.contains(&CardCode::new(COVER_UP)),
        "a found weakness does not stay in hand",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code == CardCode::new(COVER_UP))
        .expect("Cover Up resolved its Revelation into the threat area");
    assert_eq!(placed.clues, 3);
}

/// The searcher answers the search. Old Book of Lore: *"Choose an investigator
/// at your location. That investigator searches the top 3 cards of his or her
/// deck for a card, draws it…"* `glossary/Search.md`: *"When a player is
/// instructed to search for a card, that player is permitted to look at all of
/// the cards in the searched area without revealing those cards to the other
/// players."* So when the reader chooses Roland, the pick among his top 3 is
/// Roland's, and the card he picks is drawn into his hand.
#[test]
fn a_search_of_another_investigators_deck_is_answered_by_that_investigator() {
    let session = reader_beside_roland();
    let book = book(&session);
    let roland_card = session.state().investigator_card(ROLAND);
    let s = session.take(&activate_book(book)).pick(roland_card);
    assert_eq!(
        s.prompt().responder,
        Responder::Investigator(ROLAND),
        "the search of Roland's deck is Roland's to answer: {:?}",
        s.prompt(),
    );

    let s = s.pick_unanchored("Magnifying Glass");
    s.assert_at_turn_menu(INV);
    let roland = &s.state().investigators[&ROLAND];
    assert!(
        roland.hand.contains(&CardCode::new(MAGNIFYING_GLASS)),
        "Roland drew the card he picked",
    );
    assert_eq!(roland.deck.len(), 3, "one card left Roland's deck");
    assert!(
        s.state().investigators[&INV].hand.is_empty(),
        "the reader drew nothing",
    );
}

/// The reader choosing herself searches her own deck, so the pick stays hers.
#[test]
fn a_search_of_the_readers_own_deck_is_answered_by_the_reader() {
    let session = reader_beside_roland();
    let book = book(&session);
    let reader_card = session.state().investigator_card(INV);
    let s = session.take(&activate_book(book)).pick(reader_card);
    assert_eq!(s.prompt().responder, Responder::Investigator(INV));
}
