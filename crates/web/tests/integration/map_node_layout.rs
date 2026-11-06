//! Regression for #848: a map node keeps its occupancy tokens in view, and
//! stays inside its grid cell.
//!
//! A fixed-size node with `overflow: hidden` clipped them on the Parlor 01115,
//! the longest printed text of any Core location, and hid Lita Chantler once an
//! investigator stood there. The card is a header only, with its printed text
//! in a hover popover, so in a 100px row it is the token rail that sets a
//! node's height, and the Parlor, with Lita always in its rail, is the densest
//! node.
//!
//! These tests measure rather than inspect: the real `style.css` is injected and
//! the real corpus (`cards::TEST_REGISTRY`) provided, so the node is filled with the text the
//! app actually renders, and the assertion is that no token's box falls outside
//! the node's client box at solo or two-investigator density.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, GameState, GameStateBuilder, LocationId,
    Owner as CardOwner,
};
use game_core::test_support::{self, location, seat};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::{Element, HtmlElement};

use crate::common;

const PARLOR: LocationId = LocationId(5);
const HALLWAY: LocationId = LocationId(6);
const LITA: CardInstanceId = CardInstanceId(60);

/// The Parlor (01115) with Lita Chantler (01117) at it, connected to
/// `connections`, and `investigators` investigators standing in it. Real card
/// codes throughout — the real corpus is provided, so the investigator's
/// capacity lookup needs a real code.
fn parlor_board(
    investigators: usize,
    connections: impl IntoIterator<Item = LocationId>,
) -> GameStateBuilder {
    let parlor = location(PARLOR)
        .as_card("01115")
        .connections(connections)
        .cards_at_location([CardInPlay::enter_play(
            CardCode::new("01117"),
            LITA,
            CardOwner::Scenario,
        )]);
    let mut builder = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(parlor);
    // Roland Banks 01001 and Daisy Walker 01002.
    for (id, code) in (1..).zip(["01001", "01002"]).take(investigators) {
        builder = builder.with_seat(seat(id).as_card(code).at(PARLOR));
    }
    builder
}

/// Mount the board with `state`, and return the root this mount put it in
/// (each test mounts into the same document body).
async fn mount(state: GameState) -> Element {
    mount_with(state, EngineOutcome::Done).await
}

/// As [`mount`], but with a live `outcome` so the board has options to anchor.
async fn mount_with(state: GameState, outcome: EngineOutcome) -> Element {
    common::inject_style();
    common::mount_board(common::hello(state, outcome), &cards::TEST_REGISTRY)
        .await
        .root
}

/// Assert every occupancy token in the Parlor node is inside the node's client
/// box, and that the node is not scrolling content out of sight.
async fn assert_parlor_tokens_visible(investigators: usize) {
    let root = mount(parlor_board(investigators, []).build()).await;
    let node: HtmlElement = root
        .query_selector(".map-location[data-loc=\"Parlor\"]")
        .expect("query")
        .expect("the Parlor node")
        .dyn_into()
        .expect("HtmlElement");
    let tokens = node
        .query_selector_all(".inv-token, .enemy-token, .at-location-token")
        .expect("query");
    assert_eq!(
        tokens.length() as usize,
        investigators + 1, // the investigators, plus Lita
        "expected one token per investigator plus Lita's",
    );
    for i in 0..tokens.length() {
        let token: HtmlElement = tokens
            .item(i)
            .and_then(|n| n.dyn_into().ok())
            .expect("HtmlElement token");
        let label = token.text_content().unwrap_or_default();
        assert!(
            token.offset_height() > 0,
            "token {label:?} renders with no height",
        );
        let bottom = token.offset_top() + token.offset_height();
        assert!(
            bottom <= node.client_height(),
            "token {label:?} is clipped: its bottom is {bottom}px, \
             the node's client height {}px",
            node.client_height(),
        );
    }
    assert!(
        node.scroll_height() <= node.client_height(),
        "the Parlor node scrolls its content out of view: content {}px \
         in a {}px box",
        node.scroll_height(),
        node.client_height(),
    );
}

/// The reported symptom: an investigator in the Parlor pushed Lita's token past
/// the bottom of the node. With a header-only card, an investigator's token
/// joins Lita's in the rail, the part of the node that sets its height.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_one_investigator() {
    assert_parlor_tokens_visible(1).await;
}

/// Two investigators in the Parlor — the density the layout also has to hold.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_two_investigators() {
    assert_parlor_tokens_visible(2).await;
}

/// Baseline: an empty Parlor must not clip Lita either, even by a few pixels.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_an_empty_parlor() {
    assert_parlor_tokens_visible(0).await;
}

/// The node grows with its content, so the other half of "nothing is clipped" is
/// that a tall node must not grow into the grid row beneath it. The Parlor,
/// with two investigators and Lita in its rail, is the tallest node the
/// Gathering can show, and it sits one row below the Hallway (01112). Nor may
/// an open popover grow a node: it shows the printed text the card leaves out,
/// and it has to do so without pushing the node into its neighbours.
#[wasm_bindgen_test]
async fn a_tall_node_does_not_overlap_the_row_above_it() {
    let state = parlor_board(2, [])
        .with_location(location(HALLWAY).as_card("01112"))
        .build();
    let root = mount(state).await;
    let node = |name: &str| -> HtmlElement {
        root.query_selector(&format!(".map-location[data-loc=\"{name}\"]"))
            .expect("query")
            .unwrap_or_else(|| panic!("the {name} node"))
            .dyn_into()
            .expect("HtmlElement")
    };
    let (hallway, parlor) = (node("Hallway"), node("Parlor"));
    let hallway_bottom = hallway.offset_top() + hallway.offset_height();
    assert!(
        hallway_bottom <= parlor.offset_top(),
        "the Hallway node overlaps the Parlor below it: it ends at {hallway_bottom}px, \
         the Parlor starts at {}px",
        parlor.offset_top(),
    );
    // …and the Parlor itself fits in a row, so it would not overlap a row
    // beneath it either.
    let row_height = parlor.offset_top() - hallway.offset_top();
    assert!(
        parlor.offset_height() <= row_height,
        "the Parlor node is {}px tall, taller than the {row_height}px grid row",
        parlor.offset_height(),
    );

    let closed = parlor.offset_height();
    common::hover(&parlor).await;
    assert!(
        common::popover_text(&parlor).is_some(),
        "hovering the Parlor opens its popover",
    );
    assert_eq!(
        parlor.offset_height(),
        closed,
        "the open popover changed the Parlor node's height",
    );
    common::unhover(&parlor).await;
}

/// Connection lines anchor on the *card*, not on the node — the node spans card
/// plus rail, so its own centre falls in the gap between them. The short-card
/// case is the one that bites, and every card is short, being only a header:
/// without a card min-height a line would end below the card in empty space. The unrevealed Hallway is the shortest of all. Asserted by
/// measurement: every endpoint lands inside some card's box.
#[wasm_bindgen_test]
async fn every_connection_line_ends_inside_a_card() {
    let state = parlor_board(1, [HALLWAY])
        .with_location(
            location(HALLWAY)
                .as_card("01112")
                .unrevealed() // the short card
                .connections([PARLOR]),
        )
        .build();
    let root = mount(state).await;

    // Card boxes and the line, both in viewport coordinates: the line's
    // endpoints are given as percentages of the map, so they are measured
    // rather than read.
    let cards: Vec<(f64, f64, f64, f64)> = {
        let all = root
            .query_selector_all(".map-location .loc-card")
            .expect("query");
        (0..all.length())
            .filter_map(|i| all.item(i))
            .filter_map(|n| n.dyn_into::<Element>().ok())
            .map(|card| {
                let r = card.get_bounding_client_rect();
                (r.left(), r.top(), r.right(), r.bottom())
            })
            .collect()
    };
    assert_eq!(cards.len(), 2, "two nodes, two cards");

    let lines = root.query_selector_all("line.map-line").expect("query");
    assert_eq!(lines.length(), 1, "the connected pair draws one line");
    let line = lines
        .item(0)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("the line")
        .get_bounding_client_rect();
    // A line's box has its endpoints at two opposite corners; which two
    // depends on its direction, so either diagonal will do.
    let (l, t, r, b) = (line.left(), line.top(), line.right(), line.bottom());
    let in_a_card = |(x, y): (f64, f64)| {
        cards
            .iter()
            .any(|&(cl, ct, cr, cb)| x >= cl && x <= cr && y >= ct && y <= cb)
    };
    assert!(
        [[(l, t), (r, b)], [(l, b), (r, t)]]
            .iter()
            .any(|ends| ends.iter().all(|&end| in_a_card(end))),
        "the line from ({l}, {t}) to ({r}, {b}) ends outside a card; cards = {cards:?}",
    );
}

/// Reported live: a token in the rail showed the pointer cursor but did nothing
/// when clicked. The node carries the `actionable` class and spans card *and*
/// rail, while its hit-layer covers only the card — so a cursor set on the node
/// promises a click the rail can't take. Glow and cursor belong on the card.
/// An investigator token is the exception: its own click opens their tab.
#[wasm_bindgen_test]
async fn only_the_card_advertises_the_location_menu() {
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Investigate").at(OptionTarget::Location(PARLOR))],
    );
    let root = mount_with(parlor_board(1, []).build(), outcome).await;
    let node = root
        .query_selector(".map-location[data-loc=\"Parlor\"]")
        .expect("query")
        .expect("the Parlor node");
    assert!(
        node.class_name().contains("actionable"),
        "the Parlor is actionable for a location-anchored option",
    );
    let cursor = |sel: &str| -> String {
        let el = node
            .query_selector(sel)
            .expect("query")
            .unwrap_or_else(|| panic!("a {sel} in the node"));
        common::computed(&el, "cursor")
    };
    assert_eq!(cursor(".loc-card"), "pointer", "the card takes the click");
    assert_eq!(
        cursor(".at-location-token"),
        "auto",
        "a rail token must not advertise a click the hit-layer can't take",
    );
    assert_eq!(
        cursor(".inv-token"),
        "pointer",
        "an investigator token takes its own click: it opens their tab"
    );
}
