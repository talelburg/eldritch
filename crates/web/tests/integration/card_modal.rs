//! The card modal, mounted as the account answering a search: the searched
//! cards render as cards, each offered one with Draw, and the prompt strip
//! stands down (ADR 0011).
//!
//! Each prompt is the engine's own, reached through a test session over the
//! real corpus, and a click goes back through the in-process server.
#![cfg(target_arch = "wasm32")]

use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, GameStateBuilder, InvestigatorId, LocationId, UseKind,
};
use game_core::test_support::{
    self, location, seat, uses, BoardLookup as _, Printed, Seat, TestSession,
};
use leptos::task;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::Element;

use crate::common::{self, Harness};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ROLAND: &str = "01001";

const OLD_BOOK_OF_LORE: &str = "01031";
const RESEARCH_LIBRARIAN: &str = "01032";
const MEDICAL_TEXTS: &str = "01035";
const ARCANE_INITIATE: &str = "01063";
const SCRYING: &str = "01061";
const SHRIVELLING: &str = "01060";
const BEAT_COP: &str = "01018";
const MAGNIFYING_GLASS: &str = "01030";
const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";
const PERCEPTION: &str = "01090";

/// Roland at `HERE` with `deck`, top first.
fn roland(deck: &[&str]) -> Seat<Printed> {
    seat(1).as_card(ROLAND).at(HERE).deck(deck.iter().copied())
}

/// `investigator`'s turn, alone at `HERE`, as a session.
fn turn(investigator: Seat<Printed>) -> TestSession {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// Activate printed ability `ability` of `code`, in play.
fn activate(session: TestSession, code: &str, ability: u8) -> TestSession {
    let source = AbilitySource::InPlay(session.state().in_play(INV, code));
    session.take(&TurnAction::ActivateAbility {
        investigator: INV,
        source,
        address: AbilityAddress::Printed(ability),
    })
}

/// Mount the board on the prompt `session` rests at, as the account holding
/// every investigator.
async fn mount(session: TestSession) -> Harness {
    let outcome = test_support::awaiting_request(session.prompt().clone());
    Harness::mount(session.state().clone(), outcome).await
}

/// The names of the cards in the modal's own row, in order.
fn shown_names(h: &Harness) -> Vec<String> {
    common::texts_in(&h.board.root, ".card-modal > .card-modal-row .card-name")
}

/// The name of each card in the modal's row that has a verb button, with the
/// button's text.
fn offered(h: &Harness) -> Vec<(String, String)> {
    let cards = h
        .board
        .root
        .query_selector_all(".card-modal > .card-modal-row > .card-modal-card")
        .expect("query");
    (0..cards.length())
        .filter_map(|i| cards.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .filter_map(|card| {
            let verb = card.query_selector(".card-modal-verb").expect("query")?;
            Some((
                common::text_of(&card, ".card-name"),
                verb.text_content().unwrap_or_default(),
            ))
        })
        .collect()
}

fn named(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// Old Book of Lore 01031: *"That investigator searches the top 3 cards of his
/// or her deck for a card, draws it, and shuffles the remaining cards into his
/// or her deck."* The 3 cards are in the modal with Draw, nowhere else, and
/// clicking Draw takes that card into hand.
#[wasm_bindgen_test]
async fn old_book_of_lore_shows_the_top_three_with_draw_and_draws_the_one_clicked() {
    let session = turn(
        roland(&[BEAT_COP, MAGNIFYING_GLASS, EMERGENCY_CACHE, GUTS]).in_play([OLD_BOOK_OF_LORE]),
    );
    let mut h = mount(activate(session, OLD_BOOK_OF_LORE, 0)).await;

    assert_eq!(
        shown_names(&h),
        named(&["Beat Cop", "Magnifying Glass", "Emergency Cache"]),
    );
    assert_eq!(
        offered(&h),
        [
            ("Beat Cop".to_owned(), "Draw".to_owned()),
            ("Magnifying Glass".to_owned(), "Draw".to_owned()),
            ("Emergency Cache".to_owned(), "Draw".to_owned()),
        ],
    );
    assert!(
        h.absent(".prompt-strip .strip-option") && h.absent(".prompt-strip .prompt"),
        "the strip lists neither the cards nor the prompt: the modal carries them",
    );

    let draws = h
        .board
        .root
        .query_selector_all(".card-modal .card-modal-verb")
        .expect("query");
    common::click(
        &draws
            .item(1)
            .and_then(|n| n.dyn_into::<Element>().ok())
            .expect("a second Draw"),
    );
    h.pump().await;

    assert!(h.absent(".card-modal"), "answering closes the modal");
    let inv = &h.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(MAGNIFYING_GLASS)]);
    assert_eq!(inv.deck.len(), 3);
}

/// Arcane Initiate 01063: *"Search the top 3 cards of your deck for a
/// \[\[Spell\]\] card and draw it."* With one Spell among the 3, all 3 are
/// shown and only the Spell has Draw. Interactive play asks even for one
/// option, as the server does.
#[wasm_bindgen_test]
async fn arcane_initiate_with_one_spell_shows_all_three_with_draw_on_the_spell() {
    let session =
        turn(roland(&[GUTS, SHRIVELLING, PERCEPTION, EMERGENCY_CACHE]).in_play([ARCANE_INITIATE]))
            .edit_state(|state| state.interactive_acknowledge = true);
    let h = mount(activate(session, ARCANE_INITIATE, 1)).await;

    assert_eq!(
        shown_names(&h),
        named(&["Guts", "Shrivelling", "Perception"]),
    );
    assert_eq!(offered(&h), [("Shrivelling".to_owned(), "Draw".to_owned())],);
    assert!(h.absent(".prompt-strip .strip-option"));
}

/// Research Librarian 01032: *"Search your deck for a \[\[Tome\]\] asset and
/// add it to your hand. Shuffle your deck."* The Tomes have Draw, and the rest
/// of the deck folds under a count.
#[wasm_bindgen_test]
async fn research_librarian_shows_the_tomes_and_collapses_the_rest() {
    let session = turn(
        roland(&[OLD_BOOK_OF_LORE, GUTS, MEDICAL_TEXTS, PERCEPTION])
            .hand([RESEARCH_LIBRARIAN])
            .resources(5),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    let librarian = session.state().card(INV, RESEARCH_LIBRARIAN);
    let h = mount(session.pick(librarian)).await;

    assert_eq!(
        offered(&h),
        [
            ("Old Book of Lore".to_owned(), "Draw".to_owned()),
            ("Medical Texts".to_owned(), "Draw".to_owned()),
        ],
    );
    assert_eq!(
        common::text_of(&h.board.root, ".card-modal-others > summary"),
        "2 other cards searched",
    );
    assert_eq!(
        common::texts_in(&h.board.root, ".card-modal-others .card-name"),
        named(&["Guts", "Perception"]),
        "the rest of the deck can be looked at, with no Draw",
    );
    assert!(h.absent(".card-modal-others .card-modal-verb"));
    assert!(h.absent(".prompt-strip .strip-option"));
}

/// Scrying 01061 *looks at* the top 3, which is not a search: its picks stay
/// on the prompt strip, under the engine's labels, until its own modal lands.
#[wasm_bindgen_test]
async fn scryings_cards_stay_on_the_prompt_strip() {
    let session = turn(
        roland(&[GUTS, PERCEPTION, EMERGENCY_CACHE])
            .in_play([(SCRYING, uses(UseKind::Charges, 3))]),
    );
    let h = mount(activate(session, SCRYING, 0).pick_nth(0)).await;
    task::tick().await;

    assert!(h.absent(".card-modal"), "looking at is not a search");
    assert_eq!(
        common::texts_in(&h.board.root, ".dock .prompt-strip .strip-option"),
        named(&["Guts", "Perception", "Emergency Cache"]),
    );
}
