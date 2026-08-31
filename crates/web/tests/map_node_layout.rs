//! Layout regression for the map node (#848). A location node used to be a
//! fixed `NODE_W`x`NODE_H` box with `overflow: hidden`, so a node whose card
//! text was long enough — the Parlor 01115, the longest printed text of any
//! Core location — clipped its own occupancy tokens out of view. Lita Chantler
//! was invisible in a real act-3 run once an investigator stood in the Parlor.
//!
//! These tests measure rather than inspect: the real `style.css` is injected and
//! the real `cards::REGISTRY` installed, so the node is filled with the text the
//! app actually renders, and the assertion is that no token's box falls outside
//! the node's client box at solo or two-investigator density.
//!
//! Own test binary: the real registry install is first-wins per process, and
//! `tests/map.rs` installs the synthetic one.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, CardInPlay, CardInstanceId, GameStateBuilder, LocationId};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use leptos::prelude::*;
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

const PARLOR: LocationId = LocationId(5);
const LITA: CardInstanceId = CardInstanceId(60);

/// Inject `style.css` once per browser session, so measurements are taken
/// against the stylesheet the app ships rather than an unstyled DOM.
fn inject_style() {
    let doc = document();
    if doc
        .query_selector("style[data-app-css]")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let style = doc.create_element("style").expect("create style");
    style.set_attribute("data-app-css", "1").expect("attr");
    style.set_text_content(Some(include_str!("../style.css")));
    doc.head()
        .expect("head")
        .append_child(&style)
        .expect("append");
}

/// The Parlor (01115) with Lita Chantler (01117) at it, and `investigators`
/// investigators standing in it. Real card codes throughout — the real registry
/// is installed, so the investigator's capacity lookup needs a real code.
fn parlor_state(investigators: usize) -> game_core::state::GameState {
    let mut parlor = test_location(5, "Parlor");
    parlor.code = CardCode::new("01115");
    parlor.revealed = true;
    parlor
        .cards_at_location
        .push(CardInPlay::enter_play(CardCode::new("01117"), LITA));
    let mut builder = GameStateBuilder::new().with_location(parlor);
    for (i, (name, code)) in [("Roland Banks", "01001"), ("Daisy Walker", "01002")]
        .into_iter()
        .take(investigators)
        .enumerate()
    {
        let mut inv = test_investigator(u32::try_from(i).expect("small index") + 1);
        inv.name = name.to_string();
        inv.investigator_card.code = CardCode::new(code);
        builder = builder.with_investigator_at(inv, PARLOR);
    }
    builder.build()
}

/// Mount `BoardView` with `state`, tick, and return the wrapper this mount put
/// the board in (each test mounts into the same document body).
async fn mount(state: game_core::state::GameState) -> web_sys::Element {
    mount_with(state, EngineOutcome::Done).await
}

/// As [`mount`], but with a live `outcome` so the board has options to anchor.
async fn mount_with(
    state: game_core::state::GameState,
    outcome: EngineOutcome,
) -> web_sys::Element {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    inject_style();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(web::interaction::PendingOptions(pending));
        view! { <div class="layout-probe"><BoardView/></div> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
    let roots = document()
        .query_selector_all(".layout-probe")
        .expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .layout-probe root")
}

/// Assert every occupancy token in the Parlor node is inside the node's client
/// box, and that the node is not scrolling content out of sight.
async fn assert_parlor_tokens_visible(investigators: usize) {
    let root = mount(parlor_state(investigators)).await;
    let node: web_sys::HtmlElement = root
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
        let token: web_sys::HtmlElement = tokens
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
/// the bottom of the node.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_one_investigator() {
    assert_parlor_tokens_visible(1).await;
}

/// Two investigators in the Parlor — the density the layout also has to hold.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_two_investigators() {
    assert_parlor_tokens_visible(2).await;
}

/// Baseline: even an empty Parlor clipped Lita by a few pixels before the fix.
#[wasm_bindgen_test]
async fn parlor_tokens_are_visible_with_an_empty_parlor() {
    assert_parlor_tokens_visible(0).await;
}

/// The node grows with its content, so the other half of "nothing is clipped" is
/// that a tall node must not grow into the grid row beneath it. The Parlor
/// (01115, the tallest Core location) sits one row below the Hallway (01112).
#[wasm_bindgen_test]
async fn a_tall_node_does_not_overlap_the_row_above_it() {
    let mut state = parlor_state(2);
    let mut hallway = test_location(6, "Hallway");
    hallway.code = CardCode::new("01112");
    hallway.revealed = true;
    state.locations.insert(LocationId(6), hallway);
    let root = mount(state).await;
    let node = |name: &str| -> web_sys::HtmlElement {
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
}

/// Connection lines anchor on the *card*, not on the node — the node spans card
/// plus rail, so its own centre falls in the gap between them. The short-card
/// case is the one that bites: an unrevealed location is barely a header tall,
/// so without a card min-height its line would end below the card in empty
/// space. Asserted by measurement: every endpoint lands inside some card's box.
#[wasm_bindgen_test]
async fn every_connection_line_ends_inside_a_card() {
    let mut hallway = test_location(6, "Hallway");
    hallway.code = CardCode::new("01112");
    hallway.revealed = false; // the short card
    let mut state = parlor_state(1);
    state.locations.insert(LocationId(6), hallway);
    state.connect(PARLOR, LocationId(6));
    let root = mount(state).await;

    // Card boxes, in the coordinate space the SVG lines are drawn in (the `.map`
    // section): a card is positioned inside its node, and the node inside `.map`.
    let nodes = root.query_selector_all(".map-location").expect("query");
    let mut cards = Vec::new();
    for i in 0..nodes.length() {
        let node: web_sys::HtmlElement = nodes
            .item(i)
            .and_then(|n| n.dyn_into().ok())
            .expect("HtmlElement node");
        let card: web_sys::HtmlElement = node
            .query_selector(".loc-card")
            .expect("query")
            .expect("every node has a card")
            .dyn_into()
            .expect("HtmlElement");
        let (left, top) = (
            node.offset_left() + card.offset_left(),
            node.offset_top() + card.offset_top(),
        );
        cards.push((
            left,
            top,
            left + card.offset_width(),
            top + card.offset_height(),
        ));
    }

    let lines = root.query_selector_all("line.map-line").expect("query");
    assert_eq!(lines.length(), 1, "the connected pair draws one line");
    let line = lines.item(0).expect("the line");
    let coord = |name: &str| -> i32 {
        line.dyn_ref::<web_sys::Element>()
            .expect("Element")
            .get_attribute(name)
            .expect("the attribute")
            .parse()
            .expect("a number")
    };
    for (x, y) in [(coord("x1"), coord("y1")), (coord("x2"), coord("y2"))] {
        assert!(
            cards
                .iter()
                .any(|&(l, t, r, b)| x >= l && x <= r && y >= t && y <= b),
            "line endpoint ({x}, {y}) lands outside every card; cards = {cards:?}",
        );
    }
}

/// Reported live: a token in the rail showed the pointer cursor but did nothing
/// when clicked. The node carries the `actionable` class and spans card *and*
/// rail, while its hit-layer covers only the card — so a cursor set on the node
/// promises a click the rail can't take. Glow and cursor belong on the card.
#[wasm_bindgen_test]
async fn only_the_card_advertises_the_location_menu() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Investigate").at(OptionTarget::Location(PARLOR))],
    );
    let root = mount_with(parlor_state(1), outcome).await;
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
        window()
            .get_computed_style(&el)
            .expect("computed style")
            .expect("a style declaration")
            .get_property_value("cursor")
            .expect("the cursor property")
    };
    assert_eq!(cursor(".loc-card"), "pointer", "the card takes the click");
    assert_eq!(
        cursor(".at-location-token"),
        "auto",
        "a rail token must not advertise a click the hit-layer can't take",
    );
    assert_eq!(cursor(".inv-token"), "auto", "nor an investigator token");
}
