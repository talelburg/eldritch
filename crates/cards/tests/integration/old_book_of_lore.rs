//! Old Book of Lore 01031's `[action] Exhaust Old Book of
//! Lore: Choose an investigator at your location. That investigator searches
//! the top 3 cards of his or her deck for a card, draws it, and shuffles the
//! remaining cards into his or her deck.` end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Solo: the "choose an investigator at your location" target auto-binds (one
//! co-located investigator), so the only suspend is the top-3 card pick. The
//! exhaust cost is paid before the effect runs.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId,
    LocationId,
};
use game_core::test_support::{location, seat, BoardLookup as _, Seat, TestSession};

const OLD_BOOK: &str = "01031";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

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
