//! The dock's cards as tiles: a hand or in-play card shows its name, cost and
//! skill icons in its class colour, and hovering it shows the whole card.
//!
//! Real corpus cards throughout, so every name and text below is the printed
//! one, from `data/arkhamdb-snapshot/pack/core/core.json`.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{GameStateBuilder, UseKind};
use game_core::test_support::{clues, seat, uses};
use leptos::prelude::*;
use leptos::task;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::{Element, MouseEvent, MouseEventInit};

use crate::common;

/// Machete: Guardian asset, cost 3, one `[combat]` icon.
const MACHETE: &str = "01020";
/// Flashlight: Neutral asset, cost 2, "Uses (3 supplies)".
const FLASHLIGHT: &str = "01087";
/// Cover Up: Roland's signature weakness, a treachery that enters his threat
/// area "with 3 clues on it".
const COVER_UP: &str = "01007";

/// Roland Banks holding Machete, with a Flashlight down to 2 supplies in play
/// and Cover Up in his threat area; the board's root.
async fn rolands_dock() -> Element {
    let state = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .as_card("01001")
                .hand([MACHETE])
                .in_play([(FLASHLIGHT, uses(UseKind::Supplies, 2))])
                .threat_area([(COVER_UP, clues(3))]),
        )
        .build();
    common::mount_board(
        common::hello(state, EngineOutcome::Done),
        &cards::TEST_REGISTRY,
    )
    .await
    .root
}

/// The skill each of the tile's icons stands for, in order.
fn icons(tile: &Element) -> Vec<String> {
    let all = tile.query_selector_all(".skill-icon").expect("query");
    (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok()?.get_attribute("title"))
        .collect()
}

#[wasm_bindgen_test]
async fn a_hand_card_is_a_tile_and_hovering_it_shows_the_whole_card() {
    let root = rolands_dock().await;
    let slot = common::one(&root, ".dock .hand .hand-slot");
    let tile = common::one(&slot, ".tile");

    assert!(tile.class_list().contains("card--guardian"), "class border");
    assert_eq!(common::text_of(&tile, ".tile-name"), "Machete");
    assert_eq!(common::text_of(&tile, ".tile-cost"), "3");
    assert_eq!(icons(&tile), vec!["combat"]);
    let rules = "If the attacked enemy is the only enemy engaged with you, \
                 this attack deals +1 damage.";
    assert!(
        !tile.text_content().unwrap_or_default().contains(rules),
        "the tile leaves the text to the popover"
    );

    let popover = common::hovered_popover(&slot).await;
    assert!(popover.contains("Item. Weapon. Melee."), "{popover}");
    assert!(popover.contains(rules), "{popover}");
    common::unhover(&slot).await;
}

#[wasm_bindgen_test]
async fn an_in_play_card_is_a_tile_showing_its_uses() {
    let root = rolands_dock().await;
    let slot = common::one(&root, ".dock .in-play .card-slot");
    let tile = common::one(&slot, ".tile");

    assert!(tile.class_list().contains("card--neutral"), "class border");
    assert_eq!(common::text_of(&tile, ".tile-name"), "Flashlight");
    assert_eq!(common::text_of(&tile, ".tile-cost"), "2");
    assert_eq!(icons(&tile), vec!["intellect"]);
    assert_eq!(common::text_of(&tile, ".chip--live"), "2 supplies");

    let popover = common::hovered_popover(&slot).await;
    assert!(
        popover.contains("Your location gets -2 shroud for this investigation."),
        "{popover}"
    );
    common::unhover(&slot).await;
}

/// A popover opens away from the window's nearer edges, so it stays on screen
/// wherever the pointer is, even over the hand at the window's bottom row.
#[wasm_bindgen_test]
async fn a_tiles_popover_stays_on_screen_at_the_windows_corner() {
    common::inject_style();
    let root = rolands_dock().await;
    let slot = common::one(&root, ".dock .hand .hand-slot");
    let (width, height) = (
        window()
            .inner_width()
            .expect("width")
            .as_f64()
            .expect("a number"),
        window()
            .inner_height()
            .expect("height")
            .as_f64()
            .expect("a number"),
    );
    let init = MouseEventInit::new();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a window's size in CSS pixels fits an i32"
    )]
    {
        init.set_client_x(width as i32 - 5);
        init.set_client_y(height as i32 - 5);
    }
    let enter = MouseEvent::new_with_mouse_event_init_dict("mouseenter", &init).expect("event");
    slot.dispatch_event(&enter).expect("dispatch");
    task::tick().await;

    let popover = common::one(&slot, ".popover").get_bounding_client_rect();
    assert!(
        popover.left() >= 0.0
            && popover.top() >= 0.0
            && popover.right() <= width
            && popover.bottom() <= height,
        "the popover ({}, {})-({}, {}) leaves the {width}x{height} window",
        popover.left(),
        popover.top(),
        popover.right(),
        popover.bottom(),
    );
    common::unhover(&slot).await;
}

#[wasm_bindgen_test]
async fn a_weakness_tile_is_marked_as_one() {
    let root = rolands_dock().await;
    let tile = common::one(&root, ".dock .threat .tile");

    assert_eq!(common::text_of(&tile, ".tile-name"), "Cover Up");
    assert!(tile.class_list().contains("weakness"), "the corner mark");
    assert_eq!(common::text_of(&tile, ".tile-weakness"), "Weakness");
    assert_eq!(common::text_of(&tile, ".chip--live"), "clues 3");
}
