//! A search, as each account at the table sees it: the account answering gets
//! the card modal, and every other account, viewers included, sees the
//! searched cards face down on the searcher's tab, with no controls.
//!
//! `glossary/Search.md`: *"When a player is instructed to search for a card,
//! that player is permitted to look at all of the cards in the searched area
//! without revealing those cards to the other players."*
//!
//! Each game is seated as [`tabs::mount_table`]'s: Tal, [`YOU`], holds Roland
//! Banks and Agnes Baker, Sam holds Daisy Walker, and Alex is a viewer. Each
//! prompt is the engine's own, reached through a test session.
#![cfg(target_arch = "wasm32")]

use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::state::{AbilityAddress, AbilitySource, GameStateBuilder, LocationId};
use game_core::test_support::{location, seat, BoardLookup as _, TestSession};
use wasm_bindgen_test::*;

use crate::common::{self, YOU};
use crate::lobby::SAM;
use crate::tabs::{self, AGNES, ALEX, DAISY, ROLAND};

/// The Study 01111, where everyone stands.
const STUDY: LocationId = LocationId(10);

const OLD_BOOK_OF_LORE: &str = "01031";
const RESEARCH_LIBRARIAN: &str = "01032";
const MEDICAL_TEXTS: &str = "01035";
const BEAT_COP: &str = "01018";
const MAGNIFYING_GLASS: &str = "01030";
const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";

/// Roland's deck, top first: two Tomes below the top 3.
const ROLANDS_DECK: [&str; 6] = [
    BEAT_COP,
    MAGNIFYING_GLASS,
    EMERGENCY_CACHE,
    MEDICAL_TEXTS,
    GUTS,
    OLD_BOOK_OF_LORE,
];

/// The table in the Study, Roland holding [`ROLANDS_DECK`] and `rolands_hand`,
/// and Daisy with Old Book of Lore in play. Every deck holds a card, so each
/// investigator is one Old Book can choose.
fn table(rolands_hand: &[&str]) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(
            seat(ROLAND.0)
                .as_card("01001")
                .at(STUDY)
                .deck(ROLANDS_DECK)
                .hand(rolands_hand.iter().copied())
                .resources(5),
        )
        .with_seat(seat(AGNES.0).as_card("01004").at(STUDY).deck([GUTS]))
        .with_seat(
            seat(DAISY.0)
                .as_card("01002")
                .at(STUDY)
                .deck([GUTS])
                .in_play([OLD_BOOK_OF_LORE]),
        )
        .with_seating([ROLAND, DAISY, AGNES])
}

/// Daisy reads Old Book of Lore 01031 and chooses Roland: *"Choose an
/// investigator at your location. That investigator searches the top 3 cards
/// of his or her deck for a card, draws it…"* The session rests at Roland's
/// pick among his top 3.
fn daisy_has_roland_search() -> TestSession {
    let session = table(&[]).open_turn(DAISY).session();
    let book = session.state().in_play(DAISY, OLD_BOOK_OF_LORE);
    let roland = session.state().investigator_card(ROLAND);
    session
        .take(&TurnAction::ActivateAbility {
            investigator: DAISY,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .pick(roland)
}

/// Roland plays Research Librarian 01032: *"Search your deck for a \[\[Tome\]\]
/// asset and add it to your hand."* The session rests at his pick among his
/// whole deck.
fn roland_plays_research_librarian() -> TestSession {
    let session = table(&[RESEARCH_LIBRARIAN])
        .open_turn(ROLAND)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: ROLAND,
            source: PlaySource::Hand(0),
        });
    let librarian = session.state().card(ROLAND, RESEARCH_LIBRARIAN);
    session.pick(librarian)
}

/// Tal holds Roland, who answers the search, so the 3 cards are face up in his
/// modal, each with Draw, and no row stands in for them.
#[wasm_bindgen_test]
async fn the_account_answering_sees_the_three_face_up_with_draw() {
    let handle = tabs::mount_as(YOU, &daisy_has_roland_search()).await;

    assert_eq!(
        common::offered(&handle.root),
        [
            ("Beat Cop".to_owned(), "Draw".to_owned()),
            ("Magnifying Glass".to_owned(), "Draw".to_owned()),
            ("Emergency Cache".to_owned(), "Draw".to_owned()),
        ],
    );
    assert!(common::absent(&handle.root, ".choice-row"));
}

/// Sam, and a viewer, see three face-down cards on Roland's tab, being
/// searched, with no controls, no card's name, and no modal.
#[wasm_bindgen_test]
async fn every_other_account_sees_three_face_down_on_rolands_tab() {
    let session = daisy_has_roland_search();
    for you in [SAM, ALEX] {
        let handle = tabs::mount_as(you, &session).await;
        common::click_tab(&handle.root, "Roland Banks").await;

        assert_eq!(
            common::text_of(&handle.root, ".dock .choice-row .choice-row-caption"),
            "being searched",
            "{you:?}",
        );
        assert_eq!(
            common::count(&handle.root, ".dock .choice-row .deck-back"),
            3,
            "{you:?}"
        );
        assert!(
            common::absent(&handle.root, ".card-modal"),
            "{you:?}: no modal"
        );
        assert!(
            common::absent(&handle.root, ".choice-row button")
                && common::absent(&handle.root, ".choice-row .card-name"),
            "{you:?}: no controls and no card named",
        );

        common::click_tab(&handle.root, "Daisy Walker").await;
        assert!(
            common::absent(&handle.root, ".choice-row"),
            "{you:?}: only on Roland's tab"
        );
    }
}

/// Research Librarian searches Roland's whole deck: everyone else sees one
/// face-down deck, with its count.
#[wasm_bindgen_test]
async fn every_other_account_sees_a_searched_deck_as_one_with_its_count() {
    let session = roland_plays_research_librarian();
    let deck = session.state().investigators[&ROLAND].deck.len();
    for you in [SAM, ALEX] {
        let handle = tabs::mount_as(you, &session).await;
        common::click_tab(&handle.root, "Roland Banks").await;

        assert_eq!(
            common::text_of(&handle.root, ".dock .choice-row .choice-row-caption"),
            format!("being searched: {deck} cards"),
            "{you:?}",
        );
        assert_eq!(
            common::count(&handle.root, ".dock .choice-row .deck-back"),
            1,
            "{you:?}"
        );
        assert!(common::absent(&handle.root, ".card-modal"), "{you:?}");
    }
    let tal = tabs::mount_as(YOU, &session).await;
    assert_eq!(
        common::offered(&tal.root),
        [
            ("Medical Texts".to_owned(), "Draw".to_owned()),
            ("Old Book of Lore".to_owned(), "Draw".to_owned()),
        ],
        "Tal searches it",
    );
}
