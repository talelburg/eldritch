//! A pile: its top card as a header tile with a count, the pile list the
//! count opens, and the options anchored on Wendy's pile.
//!
//! Wendy Adams's discard pile is, bottom to top, Knife 01086 then Emergency
//! Cache 01088, so Emergency Cache is on top. Real corpus cards, so every name
//! and text below is the printed one, from
//! `data/arkhamdb-snapshot/pack/core/core.json`.
#![cfg(target_arch = "wasm32")]

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, EngineOutcome, OptionTarget};
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, location, seat};
use leptos::prelude::*;
use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::pile::Pile;
use web::registry::provide_cards;
use web_sys::{Element, HtmlElement, KeyboardEvent, KeyboardEventInit};

use crate::common::{self, YOU};
use crate::lobby;
use crate::tabs::{self, AGNES, ALEX, DAISY, ROLAND};

/// Knife: a Neutral asset, bottom of the pile.
const KNIFE: &str = "01086";
/// Emergency Cache: a Neutral event, on top.
const EMERGENCY_CACHE: &str = "01088";

/// Mount a pile of `top_first` labelled "Discard", and return it.
async fn mount_pile(top_first: &[&str]) -> Element {
    let cards: Vec<CardCode> = top_first.iter().copied().map(CardCode::new).collect();
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <Pile cards=cards label="Discard" empty_text="empty"/> }
    });
    task::tick().await;
    common::last_mounted(".pile")
}

/// Wendy's pile: Emergency Cache on top of Knife.
async fn wendys_pile() -> Element {
    mount_pile(&[EMERGENCY_CACHE, KNIFE]).await
}

/// Click the pile's count control, and let the list open.
async fn open_list(pile: &Element) {
    click(&common::one(pile, ".pile-count")).await;
}

/// Click `el`, and let the page react.
async fn click(el: &Element) {
    el.clone()
        .dyn_into::<HtmlElement>()
        .expect("an HTML element")
        .click();
    task::tick().await;
}

/// Whether `pile`'s list is open.
fn list_open(pile: &Element) -> bool {
    pile.query_selector(".pile-list").expect("query").is_some()
}

#[wasm_bindgen_test]
async fn a_pile_shows_its_top_card_and_a_count() {
    let pile = wendys_pile().await;

    assert_eq!(common::text_of(&pile, ".zone-label"), "Discard");
    assert_eq!(
        common::text_of(&pile, ".pile-top .pile-card-name"),
        "Emergency Cache"
    );
    assert_eq!(common::text_of(&pile, ".pile-count"), "2");
    // The tile is a header: the printed text waits for a hover.
    let text = pile.text_content().unwrap_or_default();
    assert!(
        !text.contains("Knife"),
        "the card under the top shows: {text}"
    );
    assert!(!list_open(&pile), "the list opens only on a click");
}

#[wasm_bindgen_test]
async fn hovering_the_top_card_shows_its_printed_text() {
    let pile = wendys_pile().await;

    let text = common::hovered_popover(&common::one(&pile, ".pile-top")).await;
    // Emergency Cache: "Gain 3 resources."
    assert!(text.contains("Emergency Cache"), "{text}");
    assert!(text.contains("Gain 3 resources."), "{text}");
}

#[wasm_bindgen_test]
async fn the_count_opens_the_pile_list_top_first() {
    let pile = wendys_pile().await;

    open_list(&pile).await;
    assert_eq!(
        common::texts_in(&pile, ".pile-list .pile-card-name"),
        vec!["Emergency Cache", "Knife"]
    );
    // Read-only: the list offers nothing to click but its cards' text.
    assert!(
        pile.query_selector(".pile-list button")
            .expect("query")
            .is_none(),
        "the pile list carries a control"
    );
}

#[wasm_bindgen_test]
async fn a_card_in_the_list_shows_its_text_on_hover() {
    let pile = wendys_pile().await;
    open_list(&pile).await;

    let cards = pile
        .query_selector_all(".pile-list .pile-card")
        .expect("query");
    let knife = cards
        .item(1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a second card in the list");
    let text = common::hovered_popover(&knife).await;
    assert!(text.contains("Knife"), "{text}");
    assert!(text.contains("Fight."), "{text}");
}

#[wasm_bindgen_test]
async fn a_click_outside_closes_the_pile_list() {
    let pile = wendys_pile().await;
    open_list(&pile).await;
    assert!(list_open(&pile));

    click(&common::one(&pile, ".menu-backdrop")).await;
    assert!(!list_open(&pile), "the list stayed open");
}

#[wasm_bindgen_test]
async fn escape_closes_the_pile_list() {
    let pile = wendys_pile().await;
    open_list(&pile).await;
    assert!(list_open(&pile));

    press(&pile, "Escape").await;
    assert!(!list_open(&pile), "the list stayed open");
}

#[wasm_bindgen_test]
async fn another_key_leaves_the_pile_list_open() {
    let pile = wendys_pile().await;
    open_list(&pile).await;

    press(&pile, "Enter").await;
    assert!(list_open(&pile), "a key other than Escape closed the list");
}

/// Press `key` with the focus inside `pile`: the keydown bubbles up to the
/// window, as a real key press would.
async fn press(pile: &Element, key: &str) {
    let init = KeyboardEventInit::new();
    init.set_key(key);
    init.set_bubbles(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("a keyboard event");
    pile.dispatch_event(&event).expect("dispatch the key");
    task::tick().await;
}

#[wasm_bindgen_test]
async fn an_empty_pile_reads_empty_and_cannot_be_opened() {
    let pile = mount_pile(&[]).await;

    assert_eq!(common::text_of(&pile, ".pile-empty"), "empty");
    assert!(
        pile.query_selector(".pile-count").expect("query").is_none(),
        "an empty pile offers a count control"
    );
    assert!(pile.query_selector(".pile-top").expect("query").is_none());
}

/// The Study 01111.
const STUDY: LocationId = LocationId(10);

#[wasm_bindgen_test]
async fn every_investigators_tab_shows_their_discard_pile_to_every_account() {
    // Tal holds Roland and Agnes, Sam holds Daisy, and Alex is a viewer. Only
    // Daisy's pile has cards in it.
    let game = || {
        GameStateBuilder::new()
            .with_registries(cards::TEST_REGISTRIES)
            .with_location(location(STUDY).as_card("01111"))
            .with_seat(seat(ROLAND.0).as_card("01001").at(STUDY))
            .with_seat(seat(AGNES.0).as_card("01004").at(STUDY))
            .with_seat(
                seat(DAISY.0)
                    .as_card("01002")
                    .at(STUDY)
                    .discard([KNIFE, EMERGENCY_CACHE]),
            )
            .with_seating([ROLAND, DAISY, AGNES])
    };
    for you in [YOU, ALEX] {
        let handle = tabs::mount_table(you, game(), EngineOutcome::Done).await;
        for (name, top, count) in [
            ("Roland Banks", None, None),
            ("Daisy Walker", Some("Emergency Cache"), Some("2")),
            ("Agnes Baker", None, None),
        ] {
            common::click_tab(&handle.root, name).await;
            assert_eq!(
                lobby::texts(&handle, ".dock .pile-top .pile-card-name"),
                top.into_iter().map(String::from).collect::<Vec<_>>(),
                "{name}'s top card, as account {you:?} sees it"
            );
            assert_eq!(
                lobby::texts(&handle, ".dock .pile-count"),
                count.into_iter().map(String::from).collect::<Vec<_>>(),
                "{name}'s count, as account {you:?} sees it"
            );
            if top.is_none() {
                assert_eq!(lobby::texts(&handle, ".dock .pile-empty"), vec!["empty"]);
            }
        }
    }
}

/// Wendy Adams 01005.
const WENDY: InvestigatorId = InvestigatorId(1);
/// Wendy's Amulet 01014: *"You may play the topmost event in your discard pile
/// as if it were in your hand."*
const AMULET: &str = "01014";

/// Mount the board on Wendy's open turn, the Amulet in play and her discard
/// pile `bottom_first`, at the open-turn menu the engine builds for it. Return
/// the board and the prompt's options.
async fn wendys_turn(bottom_first: [&'static str; 2]) -> (common::BoardHandle, Vec<ChoiceOption>) {
    let state = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(
            seat(WENDY.0)
                .as_card("01005")
                .at(STUDY)
                .in_play([AMULET])
                .discard(bottom_first),
        )
        .open_turn(WENDY)
        .build();
    // The engine's own prompt, so the anchor is the one it attaches.
    let seeded = test_support::TestSession::new(state, cards::TEST_REGISTRIES);
    let options = seeded.prompt().options.clone();
    let outcome = test_support::awaiting_request(seeded.prompt().clone());
    let handle = common::mount_board(
        common::hello(seeded.state().clone(), outcome),
        &cards::TEST_REGISTRY,
    )
    .await;
    (handle, options)
}

#[wasm_bindgen_test]
async fn the_amulet_play_is_chosen_on_wendys_top_card() {
    let (mut handle, options) = wendys_turn([KNIFE, EMERGENCY_CACHE]).await;
    let play = options
        .iter()
        .find(|o| o.target == Some(OptionTarget::DiscardPile(WENDY)))
        .expect("the engine offers the play on her discard pile");

    let top = common::one(&handle.root, ".dock .pile-top");
    assert!(
        top.class_name().contains("actionable"),
        "the top card glows"
    );
    click(&common::one(&top, ".menu-hit")).await;
    let items = common::texts_in(&top, ".context-menu .menu-item");
    assert_eq!(items, vec!["Play Emergency Cache (Wendy's Amulet)"]);

    click(&common::one(&top, ".context-menu .menu-item")).await;
    match handle.rx.try_recv().expect("the choice submitted a frame") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(play.id)),
        other => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn with_a_non_event_on_top_wendys_pile_offers_nothing() {
    // Emergency Cache is an event, but Knife is on top of it.
    let (handle, _) = wendys_turn([EMERGENCY_CACHE, KNIFE]).await;

    let top = common::one(&handle.root, ".dock .pile-top");
    assert!(
        !top.class_name().contains("actionable"),
        "the top card glows"
    );
    assert!(
        top.query_selector(".menu-hit").expect("query").is_none(),
        "the top card opens a menu"
    );
}
