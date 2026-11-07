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

const ANCIENT_EVILS: &str = "01166";
const CRYPT_CHILL: &str = "01167";
const OBSCURING_FOG: &str = "01168";
const ROTTING_REMAINS: &str = "01163";

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

/// The names of the cards in the modal's "placed so far" row, top first.
fn placed_names(h: &Harness) -> Vec<String> {
    common::texts_in(&h.board.root, ".card-modal-placed .card-name")
}

/// Click the verb button under the card named `name` in the modal's own row.
async fn place_next(h: &mut Harness, name: &str) {
    let card = common::named(
        &h.board.root,
        ".card-modal > .card-modal-row > .card-modal-card",
        ".card-name",
        name,
    );
    common::click(&common::one(&card, ".card-modal-verb"));
    h.pump().await;
}

/// The deck decision's branches on Scrying, in printed order.
const INVESTIGATOR_DECK: usize = 0;
const ENCOUNTER_DECK: usize = 1;

/// Scrying 01061: *"Look at the top 3 cards of any investigator's deck or the
/// encounter deck. Return them to the top of that deck in any order."* Each
/// pick places one card next from the top; the cards placed so far sit in a
/// row labelled "top", and the deck ends in the order picked.
#[wasm_bindgen_test]
async fn scrying_a_deck_places_each_card_next_and_shows_those_placed() {
    let session = turn(
        roland(&[GUTS, PERCEPTION, EMERGENCY_CACHE, BEAT_COP])
            .in_play([(SCRYING, uses(UseKind::Charges, 3))]),
    );
    let mut h = mount(activate(session, SCRYING, 0).pick_nth(INVESTIGATOR_DECK)).await;

    assert_eq!(
        offered(&h),
        [
            ("Guts".to_owned(), "Place next".to_owned()),
            ("Perception".to_owned(), "Place next".to_owned()),
            ("Emergency Cache".to_owned(), "Place next".to_owned()),
        ],
    );
    assert!(h.absent(".card-modal-placed"), "nothing is placed yet");
    assert!(
        h.absent(".prompt-strip .strip-option") && h.absent(".prompt-strip .prompt"),
        "the strip lists neither the cards nor the prompt: the modal carries them",
    );

    place_next(&mut h, "Emergency Cache").await;

    assert_eq!(placed_names(&h), named(&["Emergency Cache"]));
    assert_eq!(
        common::text_of(&h.board.root, ".card-modal-placed .card-modal-placed-label"),
        "top",
    );
    assert_eq!(
        offered(&h),
        [
            ("Guts".to_owned(), "Place next".to_owned()),
            ("Perception".to_owned(), "Place next".to_owned()),
        ],
    );
    assert!(h.absent(".prompt-strip .strip-option"));

    place_next(&mut h, "Perception").await;

    assert!(h.absent(".card-modal"), "the last card needs no pick");
    assert_eq!(
        h.state().investigators[&INV].deck,
        [EMERGENCY_CACHE, PERCEPTION, GUTS, BEAT_COP].map(CardCode::new),
    );
}

/// Scrying on the encounter deck: the encounter cards render by name and
/// text, through the card component's generic face.
#[wasm_bindgen_test]
async fn scrying_the_encounter_deck_shows_encounter_cards_by_name_and_text() {
    let session =
        turn(roland(&[GUTS]).in_play([(SCRYING, uses(UseKind::Charges, 3))])).edit_state(|state| {
            state.encounter_deck = [ANCIENT_EVILS, CRYPT_CHILL, OBSCURING_FOG, ROTTING_REMAINS]
                .map(CardCode::new)
                .into();
        });
    let mut h = mount(activate(session, SCRYING, 0).pick_nth(ENCOUNTER_DECK)).await;

    assert_eq!(
        shown_names(&h),
        named(&["Ancient Evils", "Crypt Chill", "Obscuring Fog"]),
    );
    assert!(
        common::text_of(&h.board.root, ".card-modal-card .card-text")
            .contains("Place 1 doom on the current agenda"),
        "Ancient Evils' text is on its card",
    );
    assert!(h.absent(".prompt-strip .strip-option"));

    place_next(&mut h, "Crypt Chill").await;
    assert_eq!(placed_names(&h), named(&["Crypt Chill"]));
    assert_eq!(shown_names(&h), named(&["Ancient Evils", "Obscuring Fog"]));

    place_next(&mut h, "Obscuring Fog").await;
    assert!(h.absent(".card-modal"));
    assert_eq!(
        h.state().encounter_deck.iter().cloned().collect::<Vec<_>>(),
        [CRYPT_CHILL, OBSCURING_FOG, ANCIENT_EVILS, ROTTING_REMAINS].map(CardCode::new),
    );
}
