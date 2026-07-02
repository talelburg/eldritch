//! Headless render tests for the spatial board map (#497). Mount `BoardView`,
//! feed a constructed `GameState`, assert on the rendered DOM. wasm32-only.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::fixtures::{
    awaiting_pick_single_with, test_enemy, test_investigator, test_location,
};
use game_core::{ChoiceOption, EngineOutcome, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::{document, provide_context, RwSignal, Signal, Update, With};
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;
use web_sys::Element;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `BoardView`, feed one `Hello` carrying `state`, tick, return nothing —
/// assertions query the live DOM via `document()`.
async fn mount_state(state: game_core::state::GameState) {
    game_core::test_support::install_test_registry();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
}

/// `textContent` of the map node whose `data-loc` equals `loc_name`, scoped to
/// the LAST mounted `<section class="map">` so that DOM accumulation across tests
/// does not make an earlier test's node shadow this one (same pattern as the
/// `board.rs` wasm tests).
fn node_text(loc_name: &str) -> String {
    let maps = document()
        .query_selector_all(".map")
        .expect("query_selector_all ok");
    let len = maps.length();
    if len == 0 {
        return String::new();
    }
    let last_map = maps
        .item(len - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("last .map is an Element");
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    last_map
        .query_selector(&sel)
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

/// Count of `<line class="map-line">` elements in the LAST mounted `.map`
/// section. Scoped to the last section so that DOM accumulation from earlier
/// test mounts does not carry over stale lines into a fresh assertion.
fn line_count() -> u32 {
    let maps = document().query_selector_all(".map").expect("query ok");
    let len = maps.length();
    if len == 0 {
        return 0;
    }
    let last_map = maps
        .item(len - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map is an Element");
    last_map
        .query_selector_all("line.map-line")
        .expect("query ok")
        .length()
}

#[wasm_bindgen_test]
async fn connected_locations_draw_a_line() {
    let mut state = GameStateBuilder::new()
        .with_location(test_location(10, "Hallway"))
        .with_location(test_location(11, "Attic"))
        .with_investigator(test_investigator(1))
        .build();
    state.connect(LocationId(10), LocationId(11));
    mount_state(state).await;
    assert_eq!(
        line_count(),
        1,
        "a connected pair must draw exactly one line"
    );
}

#[wasm_bindgen_test]
async fn isolated_location_draws_no_line() {
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study")) // no connections
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    assert_eq!(line_count(), 0, "an isolated location draws no lines");
}

#[wasm_bindgen_test]
async fn investigator_renders_inside_its_location_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Test Investigator 1"),
        "investigator token must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_in_detail_panel_not_in_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .with_enemy(enemy)
        .build();
    mount_state(state).await;

    // Not in the location node (engaged enemies leave the location box)…
    assert!(
        !node_text("Study").contains("Mock Ghoul"),
        "engaged enemy must NOT render in the location node; node = {:?}",
        node_text("Study"),
    );
    // …but present in the investigator detail panel (query last to avoid DOM accumulation).
    let investigators = document()
        .query_selector_all(".investigators")
        .expect("query_selector_all ok");
    let len = investigators.length();
    let panel = investigators
        .item(len.saturating_sub(1))
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default();
    assert!(
        panel.contains("Mock Ghoul"),
        "engaged enemy must render in the detail panel; panel = {panel:?}",
    );
}

#[wasm_bindgen_test]
async fn revealed_location_shows_revealed_not_unrevealed() {
    let mut loc = test_location(20, "Study Revealed");
    loc.revealed = true; // default, but explicit
    let state = GameStateBuilder::new()
        .with_location(loc)
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    let text = node_text("Study Revealed");
    assert!(
        text.contains("revealed"),
        "revealed location must contain 'revealed'; text = {text:?}"
    );
    assert!(
        !text.contains("unrevealed"),
        "revealed location must NOT contain 'unrevealed'; text = {text:?}"
    );
}

#[wasm_bindgen_test]
async fn unrevealed_location_shows_unrevealed() {
    let mut loc = test_location(21, "Parlor Unrevealed");
    loc.revealed = false;
    let state = GameStateBuilder::new()
        .with_location(loc)
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    let text = node_text("Parlor Unrevealed");
    assert!(
        text.contains("unrevealed"),
        "unrevealed location must contain 'unrevealed'; text = {text:?}"
    );
}

#[wasm_bindgen_test]
async fn unrevealed_location_hides_shroud_and_clues() {
    let mut loc = test_location(30, "Cellar Unrevealed");
    loc.revealed = false;
    loc.shroud = 4;
    loc.clues = 3;
    let state = GameStateBuilder::new()
        .with_location(loc)
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    let text = node_text("Cellar Unrevealed");
    assert!(
        text.contains("unrevealed"),
        "unrevealed location must contain 'unrevealed'; text = {text:?}"
    );
    assert!(
        !text.contains("shroud"),
        "unrevealed location must NOT contain 'shroud'; text = {text:?}"
    );
    assert!(
        !text.contains("clues"),
        "unrevealed location must NOT contain 'clues'; text = {text:?}"
    );
}

#[wasm_bindgen_test]
async fn revealed_location_shows_shroud_and_clues() {
    let mut loc = test_location(31, "Attic Revealed");
    loc.revealed = true;
    loc.shroud = 2;
    loc.clues = 5;
    let state = GameStateBuilder::new()
        .with_location(loc)
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    let text = node_text("Attic Revealed");
    assert!(
        text.contains("shroud"),
        "revealed location must contain 'shroud'; text = {text:?}"
    );
    assert!(
        text.contains("clues"),
        "revealed location must contain 'clues'; text = {text:?}"
    );
}

#[wasm_bindgen_test]
async fn unengaged_enemy_renders_inside_its_location_node() {
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = None;
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Mock Ghoul"),
        "unengaged enemy must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}

/// Mount `BoardView` with a store, an outbound channel, and a `PendingOptions`
/// signal derived from the store (as `app.rs` does), then feed one `Hello`
/// carrying `state` + `outcome`. Returns the submitted-frame receiver.
async fn mount_interactive(
    state: game_core::state::GameState,
    outcome: EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    game_core::test_support::install_test_registry();
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(web::interaction::PendingOptions(pending));
        leptos::view! { <BoardView/> }
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
    rx
}

/// A one-location ("Study", id 10) game with investigator 1 standing on it.
fn study_game() -> game_core::state::GameState {
    let mut loc = test_location(10, "Study");
    loc.revealed = true;
    loc.code = CardCode::new("01111");
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut game = GameStateBuilder::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .build();
    game.locations.insert(LocationId(10), loc);
    game
}

/// The `class` attribute of the last-mounted map node named `loc_name`.
fn node_class(loc_name: &str) -> String {
    let maps = document().query_selector_all(".map").expect("query");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map");
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    last.query_selector(&sel)
        .expect("query")
        .and_then(|el| el.get_attribute("class"))
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn actionable_location_glows_opens_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Investigate",
            OptionTarget::Location(LocationId(10)),
        )],
    );
    let mut rx = mount_interactive(study_game(), outcome).await;

    assert!(
        node_class("Study").contains("actionable"),
        "node has the actionable class"
    );

    // Clicking the node's hit-layer opens its menu (events bubble up, so the
    // hit-layer — not the node — carries the open handler).
    let maps = document().query_selector_all(".map").expect("query");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map");
    last.query_selector(".map-location[data-loc=\"Study\"] .menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    leptos::task::tick().await;

    let item = last
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item rendered");
    assert_eq!(item.text_content().unwrap_or_default(), "Investigate");

    // Clicking the item submits the anchored option.
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame was sent after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn location_without_a_matching_option_is_not_actionable() {
    // The only option anchors to a DIFFERENT location — the Study node stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Investigate",
            OptionTarget::Location(LocationId(11)),
        )],
    );
    let _ = mount_interactive(study_game(), outcome).await;
    assert!(!node_class("Study").contains("actionable"));
}

#[wasm_bindgen_test]
async fn investigator_card_glows_for_a_reaction_anchored_to_it() {
    // A reaction on the investigator card (Roland-style) anchors to that card's
    // instance; the panel now renders it as an InPlayCardView, so it glows (#539).
    let game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let iid = game
        .investigators
        .get(&InvestigatorId(1))
        .expect("investigator")
        .investigator_card
        .instance_id;
    let outcome = awaiting_pick_single_with(
        "You may trigger",
        vec![ChoiceOption::new(
            OptionId(0),
            "Trigger",
            OptionTarget::CardInstance(iid),
        )],
    );
    let _ = mount_interactive(game, outcome).await;
    let slots = document()
        .query_selector_all(".investigator-card .card-slot")
        .expect("query");
    let last = slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("an investigator-card .card-slot");
    assert!(
        last.class_name().contains("actionable"),
        "investigator card glows for a reaction anchored to it"
    );
}
