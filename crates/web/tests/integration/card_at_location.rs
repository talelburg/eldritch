//! Regression for #847: a card sitting **at** a location has an interactive
//! surface.
//!
//! The engine offers the Parlor 01115's granted Parley — *"While
//! Lita Chantler is not controlled by a player, she gains: '\[action\]:
//! **Parley.** Test \[intellect\] (4). If you succeed, take control of Lita
//! Chantler.'"* — anchored to Lita's **card instance**, not to the Parlor. An
//! inert rendering would anchor the option to a surface the client does not
//! render: unreachable rather than misplaced, the deadlock ADR 0011 names. These
//! tests pin the surface down — the rail token carries the anchor's options,
//! glows, and opens a menu.
//!
//! Location `attachments` are the same zone shape, so they are covered here too.
//!
//! Real corpus registry (`cards::TEST_REGISTRY`), so the tokens carry the
//! printed names.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, GameState, GameStateBuilder, LocationId,
    Owner as CardOwner,
};
use game_core::test_support::{self, location};
use leptos::task;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::{Element, HtmlElement};

use crate::common;

const PARLOR: LocationId = LocationId(5);
const LITA: CardInstanceId = CardInstanceId(60);
const FOG: CardInstanceId = CardInstanceId(61);

/// The Parlor with Lita Chantler at it, the investigator standing in it, and — when
/// `attached` — Obscuring Fog 01168 attached to the location.
fn parlor_state(attached: bool) -> GameState {
    let fog = attached
        .then(|| CardInPlay::enter_play(CardCode::new("01168"), FOG, CardOwner::EncounterDeck));
    let parlor = location(PARLOR)
        .as_card("01115")
        .cards_at_location([CardInPlay::enter_play(
            CardCode::new("01117"),
            LITA,
            CardOwner::Scenario,
        )])
        .attachments(fog);
    let inv = test_support::test_investigator(1);
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(parlor)
        .with_investigator_at(inv, PARLOR)
        .build()
}

/// A live open-turn prompt whose single option is anchored to `instance`.
fn option_anchored_to(instance: CardInstanceId) -> EngineOutcome {
    test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Parley: Lita Chantler")
            .at(OptionTarget::CardInstance(instance))],
    )
}

async fn mount(state: GameState, outcome: EngineOutcome) -> Element {
    common::mount_board(common::hello(state, outcome), &cards::TEST_REGISTRY)
        .await
        .root
}

fn query(root: &Element, selector: &str) -> Option<Element> {
    root.query_selector(selector).expect("query")
}

/// The reported symptom: the Parley anchored to Lita's instance reaches her own
/// token, and clicking it opens a menu offering that option.
#[wasm_bindgen_test]
async fn the_parley_anchored_to_lita_is_actionable_on_her_token() {
    let root = mount(parlor_state(false), option_anchored_to(LITA)).await;
    let token = query(&root, ".at-location-token").expect("Lita's at-location token");
    assert!(
        token
            .text_content()
            .unwrap_or_default()
            .contains("Lita Chantler"),
        "the token should carry her printed name, got {:?}",
        token.text_content(),
    );
    assert!(
        token.class_name().contains("actionable"),
        "Lita's token should be actionable for the option anchored to her, got {:?}",
        token.class_name(),
    );
    let hit: HtmlElement = query(&root, ".at-location-token .menu-hit")
        .expect("her token's hit-layer")
        .dyn_into()
        .expect("HtmlElement");
    hit.click();
    task::tick().await;
    let item = query(&root, ".context-menu .menu-item").expect("the opened menu's item");
    assert_eq!(
        item.text_content().unwrap_or_default(),
        "Parley: Lita Chantler"
    );
}

/// The Parlor's own options still route to the Parlor: an option anchored to a
/// card at a location must not light the node's card up as well.
#[wasm_bindgen_test]
async fn the_node_card_is_not_actionable_for_a_card_anchored_option() {
    let root = mount(parlor_state(false), option_anchored_to(LITA)).await;
    let node = query(&root, ".map-location[data-loc=\"Parlor\"]").expect("the Parlor node");
    assert!(
        !node
            .class_name()
            .split_whitespace()
            .any(|c| c == "actionable"),
        "the node should not claim the card-anchored option, got {:?}",
        node.class_name(),
    );
    // …but the node is still lifted, or its token's menu would render inside a
    // `z-index: 1` band and be overpainted by the nodes drawn after it.
    assert!(
        node.class_name()
            .split_whitespace()
            .any(|c| c == "has-menu"),
        "a node holding an actionable token must be lifted, got {:?}",
        node.class_name(),
    );
}

/// With no option anchored to her, Lita's token is inert — rendered, but no
/// glow and no hit-layer advertising a click that does nothing.
#[wasm_bindgen_test]
async fn an_uncontrolled_card_is_inert_without_options() {
    let root = mount(parlor_state(false), EngineOutcome::Done).await;
    let token = query(&root, ".at-location-token").expect("Lita's at-location token");
    assert!(
        !token.class_name().contains("actionable"),
        "no option is anchored to her, so the token must be inert, got {:?}",
        token.class_name(),
    );
    assert!(
        query(&root, ".at-location-token .menu-hit").is_none(),
        "an inert token must carry no hit-layer",
    );
}

/// A location's `attachments` are the same hole: rendered by nobody, so an
/// option anchored to an attachment was unreachable too.
#[wasm_bindgen_test]
async fn a_location_attachment_renders_and_carries_its_options() {
    let root = mount(parlor_state(true), option_anchored_to(FOG)).await;
    let token = query(&root, ".attachment-token").expect("the attachment's token");
    assert!(
        token
            .text_content()
            .unwrap_or_default()
            .contains("Obscuring Fog"),
        "the token should carry the attachment's printed name, got {:?}",
        token.text_content(),
    );
    assert!(
        token.class_name().contains("actionable"),
        "the attachment should be actionable for the option anchored to it, got {:?}",
        token.class_name(),
    );
}
