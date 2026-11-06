//! Headless render test for the location-card fields that come from the corpus
//! (traits / ability text / victory), which the header-only card leaves to its
//! hover popover, against the real corpus
//! (`cards::TEST_REGISTRY`). Mounts `location_map` directly (no investigator
//! panel → no `TEST_INV` capacity lookup). wasm32-only.
#![cfg(target_arch = "wasm32")]

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, ClueValue};
use cards::TEST_REGISTRIES;
use game_core::card_registry::CardRegistry;
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{CardCode, GameStateBuilder};
use game_core::test_support::location;
use leptos::prelude::document;
use leptos::{mount, task};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::map;
use web::registry::provide_cards;
use web_sys::Element;

use crate::common;

/// A probe location (ADR 0016) printed shroud 7, victory 1 and a Forced that
/// deals horror: everything an unrevealed location withholds, with a shroud
/// digit nothing else on the node prints. It stands in for no printed card.
const HIDDEN_ROOM: &str = "_lc_hidden_room";

static HIDDEN_ROOM_METADATA: LazyLock<CardMetadata> = LazyLock::new(|| CardMetadata {
    code: HIDDEN_ROOM.to_owned(),
    name: "Hidden Room".to_owned(),
    traits: vec![],
    text: Some("<b>Forced</b> - After you enter this location: Take 1 horror.".to_owned()),
    back_name: None,
    back_text: None,
    pack_code: "_mock".to_owned(),
    weakness: None,
    kind: CardKind::Location {
        shroud: 7,
        printed_clues: ClueValue::Fixed(2),
        victory: Some(1),
    },
});

fn probe_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    if code.as_str() == HIDDEN_ROOM {
        Some(&HIDDEN_ROOM_METADATA)
    } else {
        (cards::TEST_REGISTRY.metadata_for)(code)
    }
}

/// The real corpus with [`HIDDEN_ROOM`] composed in.
static PROBE_REGISTRY: CardRegistry = CardRegistry {
    metadata_for: probe_metadata_for,
    ..cards::TEST_REGISTRY
};

static PROBE_REGISTRIES: Registries = Registries {
    cards: &PROBE_REGISTRY,
    scenarios: &ScenarioRegistry::EMPTY,
};

/// The last-mounted map node whose `data-loc` equals `name`.
fn node(name: &str) -> Element {
    let maps = document().query_selector_all(".map").expect("query ok");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .map section");
    last.query_selector(&format!(".map-location[data-loc=\"{name}\"]"))
        .expect("query ok")
        .unwrap_or_else(|| panic!("a {name} node"))
}

/// Mount `game`'s map over `cards`, and return the `name` node.
async fn mount_node(
    game: game_core::state::GameState,
    cards: &'static CardRegistry,
    name: &str,
) -> Element {
    mount::mount_to_body(move || {
        provide_cards(cards);
        map::location_map(&game)
    });
    task::tick().await;
    node(name)
}

/// Hover `node` and return its popover's text, failing if none opens.
async fn hovered_popover(node: &Element) -> String {
    common::hover(node).await;
    common::popover_text(node).expect("hovering the node opens its popover")
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_text_and_victory() {
    // Attic 01113: victory 1, Forced text "After you enter the Attic: Take 1 horror."
    let game = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(1).as_card("01113"))
        .build();
    let attic = mount_node(game, &cards::TEST_REGISTRY, "Attic").await;

    // The card itself is header-only: the printed text waits for a hover.
    let face = attic.text_content().unwrap_or_default();
    assert!(
        !face.contains("horror"),
        "ability text on the card face: {face}"
    );
    assert!(
        !face.contains('\u{2605}'),
        "victory pip on the card face: {face}"
    );

    let text = hovered_popover(&attic).await;
    assert!(text.contains("\u{2605}1"), "victory pip missing: {text}");
    assert!(text.contains("horror"), "ability text missing: {text}");

    common::unhover(&attic).await;
    assert_eq!(
        common::popover_text(&attic),
        None,
        "leaving closes the popover"
    );
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_traits() {
    // Miskatonic University 01129: traits "Arkham."
    let game = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(2).as_card("01129"))
        .build();
    let node = mount_node(game, &cards::TEST_REGISTRY, "Miskatonic University").await;

    let face = node.text_content().unwrap_or_default();
    assert!(!face.contains("Arkham"), "traits on the card face: {face}");
    let text = hovered_popover(&node).await;
    assert!(text.contains("Arkham"), "traits missing: {text}");
}

#[wasm_bindgen_test]
async fn unrevealed_location_withholds_metadata() {
    // The probe prints victory 1, Forced text ("...Take 1 horror.") and shroud
    // 7, with 9 clues on it; unrevealed must withhold all of it (hidden info),
    // on the card face and in its popover alike.
    let game = GameStateBuilder::new()
        .with_registries(PROBE_REGISTRIES)
        .with_location(location(3).as_card(HIDDEN_ROOM).unrevealed().clues(9))
        .build();
    let node = mount_node(game, &PROBE_REGISTRY, "Hidden Room").await;

    let popover = hovered_popover(&node).await;
    assert!(
        popover.contains("Hidden Room"),
        "the popover still names the location: {popover}"
    );
    // The node's text now includes the open popover's.
    let text = node.text_content().unwrap_or_default();
    assert!(
        !text.contains('\u{2605}'),
        "victory must be withheld: {text}"
    );
    assert!(
        !text.contains("horror"),
        "ability text must be withheld: {text}"
    );
    // An unrevealed location has no shroud or clue value to print at all
    // (RR glossary, "Location Cards"): the header shows `?` placeholders.
    assert!(!text.contains('7'), "shroud value must be withheld: {text}");
    assert!(!text.contains('9'), "clue value must be withheld: {text}");
}
