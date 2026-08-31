//! The interactive surface for a card sitting **at** a location (#847).
//!
//! Since #772 the engine offers the Parlor 01115's granted Parley — *"While
//! Lita Chantler is not controlled by a player, she gains: '[action]:
//! **Parley.** Test [intellect] (4). If you succeed, take control of Lita
//! Chantler.'"* — anchored to Lita's **card instance**, not to the Parlor. The
//! map rendered her as an inert `<div>`, so the option was anchored to a
//! surface the client did not render: unreachable rather than misplaced, the
//! deadlock ADR 0011 names. These tests pin the surface down — the rail token
//! carries the anchor's options, glows, and opens a menu.
//!
//! Location `attachments` are the same zone shape and were rendered by nobody
//! at all, so they are covered here too.
//!
//! Real corpus registry, so the tokens carry the printed names; own test binary,
//! since the registry install is first-wins per process.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, CardInPlay, CardInstanceId, GameStateBuilder, LocationId};
use game_core::test_support::fixtures::{
    awaiting_pick_single_with, test_investigator, test_location,
};
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
const FOG: CardInstanceId = CardInstanceId(61);

/// The Parlor with Lita Chantler at it, Roland standing in it, and — when
/// `attached` — Obscuring Fog 01168 attached to the location.
fn parlor_state(attached: bool) -> game_core::state::GameState {
    let mut parlor = test_location(5, "Parlor");
    parlor.code = CardCode::new("01115");
    parlor.revealed = true;
    parlor
        .cards_at_location
        .push(CardInPlay::enter_play(CardCode::new("01117"), LITA));
    if attached {
        parlor
            .attachments
            .push(CardInPlay::enter_play(CardCode::new("01168"), FOG));
    }
    // The real registry is installed, so the investigator card must be a real
    // code — the panel's capacity lookup reads it.
    let mut inv = test_investigator(1);
    inv.name = "Roland Banks".into();
    inv.investigator_card.code = CardCode::new("01001");
    GameStateBuilder::new()
        .with_location(parlor)
        .with_investigator_at(inv, PARLOR)
        .build()
}

/// A live open-turn prompt whose single option is anchored to `instance`.
fn option_anchored_to(instance: CardInstanceId) -> EngineOutcome {
    awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Parley: Lita Chantler")
            .at(OptionTarget::CardInstance(instance))],
    )
}

async fn mount(state: game_core::state::GameState, outcome: EngineOutcome) -> web_sys::Element {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(web::interaction::PendingOptions(pending));
        view! { <div class="cal-root"><BoardView/></div> }
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
    let roots = document().query_selector_all(".cal-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .cal-root")
}

fn query(root: &web_sys::Element, selector: &str) -> Option<web_sys::Element> {
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
    let hit: web_sys::HtmlElement = query(&root, ".at-location-token .menu-hit")
        .expect("her token's hit-layer")
        .dyn_into()
        .expect("HtmlElement");
    hit.click();
    leptos::task::tick().await;
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
