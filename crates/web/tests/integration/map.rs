//! Headless render tests for the spatial board map.
//!
//! Mount `BoardView`, feed a constructed `GameState`, assert on the rendered
//! DOM. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, enemy, location};
use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::{Element, HtmlElement};

use crate::common::{self, BoardHandle};

/// Mount the board on a `Hello` carrying `state`, and return its root.
async fn mount_state(state: GameState) -> Element {
    common::mount_board(
        common::hello(state, EngineOutcome::Done),
        &test_support::TEST_REGISTRY,
    )
    .await
    .root
}

/// The `<section class="map">` under `root`.
fn map_in(root: &Element) -> Element {
    root.query_selector(".map")
        .expect("query ok")
        .expect("a .map section")
}

/// `textContent` of the map node whose `data-loc` equals `loc_name`, in the
/// map under `root`.
fn node_text(root: &Element, loc_name: &str) -> String {
    map_in(root)
        .query_selector(&format!(".map-location[data-loc=\"{loc_name}\"]"))
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

/// Count of `<line class="map-line">` elements in the map under `root`.
fn line_count(root: &Element) -> u32 {
    map_in(root)
        .query_selector_all("line.map-line")
        .expect("query ok")
        .length()
}

#[wasm_bindgen_test]
async fn connected_locations_draw_a_line() {
    let state = GameStateBuilder::new()
        .with_location(location(10).name("Hallway").connections([LocationId(11)]))
        .with_location(location(11).name("Attic").connections([LocationId(10)]))
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert_eq!(
        line_count(&root),
        1,
        "a connected pair must draw exactly one line"
    );
}

#[wasm_bindgen_test]
async fn isolated_location_draws_no_line() {
    let state = GameStateBuilder::new()
        .with_location(location(10).name("Study")) // no connections
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert_eq!(line_count(&root), 0, "an isolated location draws no lines");
}

#[wasm_bindgen_test]
async fn investigator_renders_inside_its_location_node() {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let state = GameStateBuilder::new()
        .with_location(location(10).name("Study"))
        .with_investigator(inv)
        .build();
    let root = mount_state(state).await;
    assert!(
        node_text(&root, "Study").contains("Test Investigator 1"),
        "investigator token must render inside its location node; Study node = {:?}",
        node_text(&root, "Study"),
    );
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_in_detail_panel_not_in_node() {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let state = GameStateBuilder::new()
        .with_location(location(10).name("Study"))
        .with_investigator(inv)
        .with_enemy(
            enemy(100)
                .name("Mock Ghoul")
                .engaged_with(InvestigatorId(1)),
        )
        .build();
    let root = mount_state(state).await;

    // Not in the location node (engaged enemies leave the location box)…
    assert!(
        !node_text(&root, "Study").contains("Mock Ghoul"),
        "engaged enemy must NOT render in the location node; node = {:?}",
        node_text(&root, "Study"),
    );
    // …but present in the investigator detail panel.
    let panel = root
        .query_selector(".investigators")
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default();
    assert!(
        panel.contains("Mock Ghoul"),
        "engaged enemy must render in the detail panel; panel = {panel:?}",
    );
}

/// The `class` attribute of the shroud / clue badge in the node
/// named `loc_name`. The header prints numerals only, so what a badge
/// *is* lives in its class, not in its text.
fn badge_class(root: &Element, loc_name: &str, which: &str) -> String {
    let sel = format!(".map-location[data-loc=\"{loc_name}\"] .loc-head .badge:{which}-child");
    map_in(root)
        .query_selector(&sel)
        .expect("query")
        .and_then(|el| el.get_attribute("class"))
        .unwrap_or_default()
}

/// Text of the shroud badge (first header column) and the clue badge (last).
fn header_badges(root: &Element, loc_name: &str) -> (String, String) {
    let sel = format!(".map-location[data-loc=\"{loc_name}\"] .loc-head .badge");
    let badges = map_in(root).query_selector_all(&sel).expect("query");
    assert_eq!(badges.length(), 2, "a header carries exactly two badges");
    let text = |i| {
        badges
            .item(i)
            .and_then(|n| n.text_content())
            .unwrap_or_default()
    };
    (text(0), text(1))
}

#[wasm_bindgen_test]
async fn revealed_location_shows_shroud_and_clues_as_badges() {
    let state = GameStateBuilder::new()
        .with_location(location(31).name("Attic Revealed").shroud(2).clues(5))
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert_eq!(
        header_badges(&root, "Attic Revealed"),
        ("2".to_string(), "5".to_string()),
        "shroud is the left badge and clues the right",
    );
    assert!(
        badge_class(&root, "Attic Revealed", "first").contains("badge--shroud"),
        "the left badge is the shroud badge",
    );
    assert!(
        badge_class(&root, "Attic Revealed", "last").contains("badge--clues"),
        "the right badge is the clue badge",
    );
    // The words are gone from the card's face — the badges carry them as titles.
    let text = node_text(&root, "Attic Revealed");
    assert!(
        !text.contains("shroud"),
        "shroud label not deleted: {text:?}"
    );
    assert!(!text.contains("clues"), "clues label not deleted: {text:?}");
}

/// A location with no clues keeps its badge — faded — so the slot never moves
/// and "no clues here" never looks like "no clue slot".
#[wasm_bindgen_test]
async fn a_revealed_location_with_no_clues_keeps_a_faded_clue_badge() {
    let state = GameStateBuilder::new()
        .with_location(location(32).name("Empty Hallway").clues(0))
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert_eq!(header_badges(&root, "Empty Hallway").1, "0");
    let class = badge_class(&root, "Empty Hallway", "last");
    assert!(
        class.contains("badge--clues") && class.contains("is-zero"),
        "a zero clue count keeps a faded clue badge; class = {class:?}",
    );
}

/// An unrevealed location has NO shroud or clue value to show — the printed
/// unrevealed side is "the side with no shroud value and/or clue value"
/// (RR glossary, "Location Cards"). Both badges are `?` placeholders, never the
/// revealed side's numbers.
#[wasm_bindgen_test]
async fn unrevealed_location_shows_placeholders_not_its_values() {
    let state = GameStateBuilder::new()
        .with_location(
            location(30)
                .name("Cellar Unrevealed")
                .shroud(4)
                .clues(3)
                .unrevealed(),
        )
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert_eq!(
        header_badges(&root, "Cellar Unrevealed"),
        ("?".to_string(), "?".to_string()),
        "an unrevealed location shows placeholders in both slots",
    );
    for which in ["first", "last"] {
        let class = badge_class(&root, "Cellar Unrevealed", which);
        assert!(
            class.contains("badge--unknown"),
            "the {which} badge is an unknown placeholder; class = {class:?}",
        );
    }
    let text = node_text(&root, "Cellar Unrevealed");
    assert!(!text.contains('4'), "shroud value leaked: {text:?}");
    assert!(!text.contains('3'), "clue value leaked: {text:?}");
}

/// The node is styled face-down by class, not by a printed "unrevealed" word.
#[wasm_bindgen_test]
async fn unrevealed_location_is_marked_by_class() {
    let state = GameStateBuilder::new()
        .with_location(location(21).name("Parlor Unrevealed").unrevealed())
        .with_investigator(test_support::test_investigator(1))
        .build();
    let root = mount_state(state).await;
    assert!(
        node_class(&root, "Parlor Unrevealed").contains("unrevealed"),
        "unrevealed node must carry the class; class = {:?}",
        node_class(&root, "Parlor Unrevealed"),
    );
    assert!(
        !node_text(&root, "Parlor Unrevealed").contains("unrevealed"),
        "the 'unrevealed' text label is gone; text = {:?}",
        node_text(&root, "Parlor Unrevealed"),
    );
}

#[wasm_bindgen_test]
async fn unengaged_enemy_renders_inside_its_location_node() {
    let state = GameStateBuilder::new()
        .with_location(location(10).name("Study"))
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy(100).name("Mock Ghoul").at(LocationId(10)))
        .build();
    let root = mount_state(state).await;
    assert!(
        node_text(&root, "Study").contains("Mock Ghoul"),
        "unengaged enemy must render inside its location node; Study node = {:?}",
        node_text(&root, "Study"),
    );
}

/// Mount the board on a `Hello` carrying `state` + `outcome`, with the
/// submitted-frame receiver on the handle.
async fn mount_interactive(state: GameState, outcome: EngineOutcome) -> BoardHandle {
    common::mount_board(common::hello(state, outcome), &test_support::TEST_REGISTRY).await
}

/// A one-location game — the Study 01111, id 10 — with investigator 1
/// standing on it.
fn study_game() -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(10).as_card("01111"))
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .build()
}

/// The `class` attribute of the map node under `root` named `loc_name`.
fn node_class(root: &Element, loc_name: &str) -> String {
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    map_in(root)
        .query_selector(&sel)
        .expect("query")
        .and_then(|el| el.get_attribute("class"))
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn actionable_location_glows_opens_menu_and_submits() {
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Investigate")
            .at(OptionTarget::Location(LocationId(10)))],
    );
    let mut board = mount_interactive(study_game(), outcome).await;
    let root = board.root.clone();

    assert!(
        node_class(&root, "Study").contains("actionable"),
        "node has the actionable class"
    );

    // Clicking the node's hit-layer opens its menu (events bubble up, so the
    // hit-layer — not the node — carries the open handler).
    let last = map_in(&root);
    last.query_selector(".map-location[data-loc=\"Study\"] .menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    task::tick().await;

    let item = last
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a menu item rendered");
    assert_eq!(item.text_content().unwrap_or_default(), "Investigate");

    // Clicking the item submits the anchored option.
    item.click();
    task::tick().await;
    let msg = board.rx.try_recv().expect("a frame was sent after tick");
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
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Investigate")
            .at(OptionTarget::Location(LocationId(11)))],
    );
    let root = mount_interactive(study_game(), outcome).await.root;
    assert!(!node_class(&root, "Study").contains("actionable"));
}

#[wasm_bindgen_test]
async fn investigator_card_glows_for_a_reaction_anchored_to_it() {
    // A reaction on the investigator card (Roland-style) anchors to that card's
    // instance; the panel renders it as an InPlayCardView, so it glows.
    let game = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let iid = game
        .investigators
        .get(&InvestigatorId(1))
        .expect("investigator")
        .investigator_card
        .instance_id;
    let outcome = test_support::awaiting_pick_single_with(
        "You may trigger",
        vec![ChoiceOption::new(OptionId(0), "Trigger").at(OptionTarget::CardInstance(iid))],
    );
    let root = mount_interactive(game, outcome).await.root;
    let last = root
        .query_selector(".investigator-card .card-slot")
        .expect("query")
        .expect("an investigator-card .card-slot");
    assert!(
        last.class_name().contains("actionable"),
        "investigator card glows for a reaction anchored to it"
    );
}
