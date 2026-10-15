//! The Necronomicon (John Dee Translation) 01009, Daisy Walker's signature
//! asset weakness, end-to-end against the real registry.
//!
//! ```text
//! Revelation - Put The Necronomicon into play in your threat area, with 3
//!   horror on it. It cannot leave play while it has 1 or more horror on it.
//! ```
//!
//! This file covers the Revelation reaching the board (#1090), making room in
//! the hand slots as it does (#1097). `glossary/Weakness.md`:
//!
//! > When an investigator draws a weakness with a player cardtype *(for
//! > example, an asset, an event, or a skill weakness)*, resolve any Revelation
//! > effects on the card, and add it to that investigator's hand.
//!
//! > If a weakness enters an investigator's hand in a manner that did not
//! > involve drawing the card, that investigator must resolve the card
//! > (including any Revelation abilities) as if he or she had just drawn it.
//!
//! And the 01009 ruling (<https://arkhamdb.com/card/01009>): *"If you add this
//! to your hand without drawing (e.g. via Research Librarian), the
//! **Revelation** ability still triggers."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::CardType;
use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, GameStateBuilder, InvestigatorId, LocationId, Owner,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const NECRONOMICON: &str = "01009";
const OLD_BOOK: &str = "01031"; // Item. Tome. — searches the top 3
const LIBRARIAN: &str = "01032"; // searches the deck for a Tome asset
const GUTS: &str = "01089"; // a skill: filler that is neither weakness nor Tome

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// `investigator`'s turn, alone at `HERE`, with no enemies (so the Draw
/// action's attacks of opportunity are empty and it resolves synchronously).
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// Take the `position`th eligible card of a deck search. Sent raw for the
/// reason `research_librarian.rs` gives: a deck search's options are cards in
/// a deck, with no board anchor a session step could name.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

/// The Necronomicon is in `INV`'s threat area with 3 horror, owned by its
/// bearer, and not in hand; it entered without any harm being dealt.
fn assert_entered_with_three_horror(session: &TestSession) {
    let inv = &session.state().investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == NECRONOMICON),
        "a Revelation that puts the card into play skips the hand",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == NECRONOMICON)
        .expect("The Necronomicon enters its bearer's threat area");
    assert_eq!(
        placed.accumulated_horror, 3,
        "\"with 3 horror on it\" is placed as it enters",
    );
    assert_eq!(placed.owner, Owner::Investigator(INV));
    assert_event!(
        session.events(),
        Event::CardEnteredThreatArea { code, .. } if code.as_str() == NECRONOMICON
    );
    // The horror is placed, not dealt: no harm event fires, and the
    // investigator takes none.
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

/// A searched-for Necronomicon's Revelation resolves once the search has
/// finished: the deck shuffles first, then the card enters play.
fn assert_resolved_after_the_shuffle(session: &TestSession) {
    let events = session.events();
    let shuffled = events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("the search shuffles the deck");
    let entered = events
        .iter()
        .position(|e| matches!(e, Event::CardEnteredThreatArea { .. }))
        .expect("The Necronomicon enters the threat area");
    assert!(
        shuffled < entered,
        "the Revelation resolves after the shuffle: {events:?}",
    );
}

#[test]
fn drawing_it_puts_it_into_the_threat_area_with_three_horror() {
    let session = board(seat(1).at(HERE).actions(3).deck([NECRONOMICON, GUTS, GUTS]))
        .take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    // The reveal announces the card's own type: an asset, not a treachery.
    assert_event!(
        session.events(),
        Event::CardRevealed { code, card_type: CardType::Asset, .. }
            if code.as_str() == NECRONOMICON
    );
}

/// Research Librarian 01032 (*"Search your deck for a Tome asset and add it to
/// your hand"*) is the case the 01009 ruling names: the Revelation fires, and
/// it resolves after the search has shuffled the deck.
#[test]
fn fetched_by_research_librarian_the_revelation_still_fires() {
    let session = board(
        seat(1)
            .at(HERE)
            .hand([LIBRARIAN])
            .deck([GUTS, NECRONOMICON, GUTS]),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    });
    // The Librarian's EnteredPlay reaction: fire it. One eligible Tome, so
    // the search auto-takes it.
    let librarian = session.state().card(INV, LIBRARIAN);
    let session = session.pick(librarian);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CardSearchedToHand { code, .. } if code.as_str() == NECRONOMICON
    );
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
}

/// Old Book of Lore 01031 (*"searches the top 3 cards of his or her deck for a
/// card, draws it"*) fetching it fires the Revelation too.
#[test]
fn fetched_by_old_book_of_lore_the_revelation_still_fires() {
    let session =
        board(
            seat(1)
                .at(HERE)
                .in_play([OLD_BOOK])
                .deck([NECRONOMICON, GUTS, GUTS, GUTS]),
        );
    let book = session.state().in_play(INV, OLD_BOOK);
    // Three eligible cards in the top 3: the pick suspends. The Necronomicon
    // is the first of them, in deck order.
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .apply(take_searched(0));

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CardSearchedToHand { code, .. } if code.as_str() == NECRONOMICON
    );
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
}

// ---- Making room as it enters the threat area (#1097) ----
//
// `glossary/Put_into_Play.md`: *"Unless otherwise stated by the put into play
// ability, cards that enter play in this manner must do so in a play area
// that satisfies the standard game rules associated with playing or drawing
// (for encounter cards) that card."* The Necronomicon holds a hand slot from
// the threat area (`glossary/Slots.md`: *"Each asset in an investigator's
// play area or threat area with a slot symbol is held in a slot of that
// type."*), so entering over the limit makes room: *"the investigator must
// choose and discard other assets under his or her control simultaneously
// with the new asset entering the slot."*

const DAISY: &str = "01002";
const TOTE_BAG: &str = "01008"; // two Tome-only hand slots
const MAGNIFYING_GLASS: &str = "01030"; // Item. Tool. — single Hand
const FLASHLIGHT: &str = "01087"; // Item. Tool. — single Hand
const COVER_UP: &str = "01007"; // Task. — a weakness treachery, no slot symbol

/// Daisy draws `top` off her deck from the turn menu, with `in_play` already
/// in her play area.
fn daisy_draws(top: &str, in_play: &[&str]) -> TestSession {
    board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .actions(3)
            .in_play(in_play.iter().copied())
            .deck([top, GUTS, GUTS]),
    )
    .take(&TurnAction::Draw { investigator: INV })
}

fn codes_in_play(session: &TestSession) -> Vec<&str> {
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .map(|c| c.code.as_str())
        .collect()
}

#[test]
fn entering_over_the_hand_limit_asks_which_hand_asset_to_discard() {
    let session = daisy_draws(NECRONOMICON, &[MAGNIFYING_GLASS, FLASHLIGHT]);
    let glass = session.state().in_play(INV, MAGNIFYING_GLASS);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(glass)),
            session.offered_at(&OptionTarget::CardInstance(flashlight)),
        ),
        (1, 1),
        "expected a make-room prompt offering both hand assets, got {:?}",
        session.prompt(),
    );
    assert!(
        session.state().investigators[&INV].threat_area.is_empty(),
        "the card enters only once room is made",
    );

    let session = session.pick(OptionTarget::CardInstance(glass));

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(codes_in_play(&session), vec![FLASHLIGHT]);
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(MAGNIFYING_GLASS)],
    );
}

/// The same prompt pauses a Revelation under a deck search: Research Librarian
/// 01032 fetches it, the search shuffles, and the make-room prompt comes
/// before the card enters.
#[test]
fn fetched_over_the_hand_limit_it_makes_room_after_the_search() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .resources(5)
            .in_play([MAGNIFYING_GLASS, FLASHLIGHT])
            .hand([LIBRARIAN])
            .deck([GUTS, NECRONOMICON, GUTS]),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    });
    let librarian = session.state().card(INV, LIBRARIAN);
    let session = session.pick(librarian);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    assert_eq!(
        session.offered_at(&OptionTarget::CardInstance(flashlight)),
        1,
        "expected the make-room prompt, got {:?}",
        session.prompt(),
    );

    let session = session.pick(OptionTarget::CardInstance(flashlight));

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS, LIBRARIAN]);
}

#[test]
fn a_free_hand_slot_takes_it_without_a_prompt() {
    let session = daisy_draws(NECRONOMICON, &[MAGNIFYING_GLASS]);

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS]);
    assert!(session.state().investigators[&INV].discard.is_empty());
}

/// Daisy's Tote Bag 01008: *"You have 2 additional hand slots, which can only
/// be used to hold \[\[Tome\]\] assets."* The Necronomicon is a Tome, so it
/// fits there with both default hand slots full.
#[test]
fn a_free_tote_bag_slot_takes_it_without_a_prompt() {
    let session = daisy_draws(NECRONOMICON, &[TOTE_BAG, MAGNIFYING_GLASS, FLASHLIGHT]);

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(
        codes_in_play(&session),
        vec![TOTE_BAG, MAGNIFYING_GLASS, FLASHLIGHT]
    );
    assert!(session.state().investigators[&INV].discard.is_empty());
}

/// A threat-area card with no slot symbol holds no slot, so it never makes
/// room, however full the hand slots are. Cover Up 01007: *"Revelation - Put
/// Cover Up into play in your threat area, with 3 clues on it."*
#[test]
fn a_slotless_threat_area_card_never_prompts() {
    let session = daisy_draws(COVER_UP, &[MAGNIFYING_GLASS, FLASHLIGHT]);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    let cover_up = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up enters the threat area");
    assert_eq!(cover_up.clues, 3);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS, FLASHLIGHT]);
    assert!(inv.discard.is_empty());
}
