//! Headless render tests for `BoardView` (P6.5). Feed a constructed
//! `GameState` through the store and assert on the rendered DOM.
//! wasm32-only (browser DOM); native jobs skip this file.
#![cfg(target_arch = "wasm32")]

use game_core::state::GameStateBuilder;
use game_core::state::{Act, Agenda, CardCode, Phase};
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

fn body_html() -> String {
    leptos::prelude::document()
        .body()
        .expect("body")
        .inner_html()
}

/// Mount `BoardView` against a fresh store and feed it one `Hello`
/// carrying `state`. Ticks once so CSR effects flush, then returns the
/// rendered body HTML.
async fn render_state(state: game_core::state::GameState) -> String {
    // Rendering an investigator panel reads `max_health()` / `max_sanity()`,
    // which resolve the investigator card's capacity from the registry (#448).
    // Install the synthetic `TEST_INV` (8/8) registry so those reads succeed.
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
    body_html()
}

#[wasm_bindgen_test]
async fn phase_bar_renders_phase_round_act_agenda() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_round(3)
        .build();
    state.act_deck = vec![Act {
        code: CardCode("_test_act".into()),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode("_test_agenda".into()),
        doom_threshold: 5,
        resolution: None,
    }];
    state.agenda_doom = 1;

    let html = render_state(state).await;

    assert!(html.contains("Investigation"), "phase missing: {html}");
    assert!(
        html.contains("round 3") || html.contains("Round 3"),
        "round missing: {html}"
    );
    assert!(
        html.contains("doom 1/5") || html.contains("1/5"),
        "agenda doom missing: {html}"
    );
    assert!(
        html.contains("clues 0/2") || html.contains("0/2"),
        "act threshold missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn map_renders_location_name_shroud_clues() {
    let mut loc = test_location(7, "Rivertown");
    loc.shroud = 3;
    loc.clues = 2;
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location(loc)
        .build();

    let html = render_state(state).await;

    assert!(html.contains("Rivertown"), "location name missing: {html}");
    assert!(html.contains("shroud 3"), "shroud missing: {html}");
    assert!(html.contains("clues 2"), "location clues missing: {html}");
}

#[wasm_bindgen_test]
async fn investigators_panel_renders_stats_and_hand() {
    use game_core::state::{CardCode, CardInPlay, CardInstanceId};

    let mut inv = test_investigator(1);
    inv.name = "Roland Banks".to_string();
    inv.investigator_card.accumulated_damage = 2; // 2/8
    inv.investigator_card.accumulated_horror = 1; // 1/8
    inv.clues = 3;
    inv.resources = 4;
    inv.actions_remaining = 2;
    inv.hand = vec![
        CardCode::new("_synth_fast_event"),
        CardCode::new("_synth_treachery"),
    ];
    inv.cards_in_play = vec![CardInPlay::enter_play(
        CardCode::new("_synth_asset"),
        CardInstanceId(0),
    )];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;

    assert!(html.contains("Roland Banks"), "name missing: {html}");
    assert!(html.contains("2/8"), "health damage missing: {html}");
    assert!(html.contains("1/8"), "horror missing: {html}");
    assert!(html.contains("actions 2"), "actions missing: {html}");
    assert!(html.contains("resources 4"), "resources missing: {html}");
    assert!(html.contains("clues 3"), "clues missing: {html}");
    assert!(
        html.contains("_synth_fast_event"),
        "hand card missing: {html}"
    );
    assert!(
        html.contains("_synth_treachery"),
        "hand card missing: {html}"
    );
    assert!(html.contains("In play"), "in-play heading missing: {html}");
    assert!(
        html.contains("_synth_asset"),
        "in-play card missing: {html}"
    );
    // In-play assets now render as Card rectangles in their own card-row.
    let in_play_cards = leptos::prelude::document()
        .query_selector_all(".in-play .card-row .card")
        .expect("query_selector_all");
    assert!(
        in_play_cards.length() >= 1,
        "in-play should render a Card: {html}"
    );
    // Hand cards now render as Card rectangles (fallback to raw code without
    // metadata in the test registry).
    let cards = leptos::prelude::document()
        .query_selector_all(".card-row .card")
        .expect("query_selector_all");
    assert!(
        cards.length() >= 1,
        "hand should render Card rectangles: {html}"
    );
}

#[wasm_bindgen_test]
async fn empty_board_renders_placeholder_without_panels() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    leptos::task::tick().await;

    // Scope to only the last mounted <section class="board"> so that
    // accumulated DOM from earlier tests does not pollute this assertion.
    let document = leptos::prelude::document();
    let boards = document
        .query_selector_all(".board")
        .expect("query_selector_all");
    let last = boards
        .item(boards.length() - 1)
        .expect("at least one .board section");
    let html = last
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html();

    assert!(
        html.contains("&lt;no game&gt;"),
        "placeholder missing: {html}"
    );
    assert!(
        !html.contains("Investigators"),
        "panels should be absent: {html}"
    );
    assert!(
        !html.contains("Locations"),
        "panels should be absent: {html}"
    );
}

/// The last mounted `.resolution` banner's inner HTML (DOM accumulates
/// across tests on the shared page — scope to the latest subtree).
fn last_resolution_html() -> String {
    let banners = leptos::prelude::document()
        .query_selector_all(".resolution")
        .expect("query_selector_all");
    banners
        .item(banners.length() - 1)
        .expect("at least one .resolution banner")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html()
}

#[wasm_bindgen_test]
async fn resolution_banner_renders_won() {
    use game_core::Resolution;
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    state.resolution = Some(Resolution::Won { id: "demo".into() });

    let _ = render_state(state).await;

    let html = last_resolution_html();
    assert!(html.contains("won"), "won banner text missing: {html}");
    assert!(html.contains("demo"), "won id missing: {html}");
}

#[wasm_bindgen_test]
async fn resolution_banner_renders_lost() {
    use game_core::Resolution;
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    state.resolution = Some(Resolution::Lost {
        reason: "cultist-surge".into(),
    });

    let _ = render_state(state).await;

    let html = last_resolution_html();
    assert!(html.contains("lost"), "lost banner text missing: {html}");
    assert!(
        html.contains("cultist-surge"),
        "lost reason missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn version_mismatch_status_renders_actionable_message() {
    use web::store::ConnStatus;
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| s.status = ConnStatus::VersionMismatch);
    leptos::task::tick().await;

    // Scope to the last mounted .status line (DOM accumulates across tests).
    let lines = leptos::prelude::document()
        .query_selector_all(".status")
        .expect("query_selector_all");
    let html = lines
        .item(lines.length() - 1)
        .expect("at least one .status line")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html();

    assert!(
        html.contains("version mismatch"),
        "status line must name the version mismatch: {html}"
    );
    assert!(
        html.contains("restart the server"),
        "status line must tell the user what to do: {html}"
    );
}

#[wasm_bindgen_test]
async fn map_and_investigators_are_inside_board_main() {
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_location(test_location(1, "Study"))
        .build();
    let _ = render_state(state).await;

    // Scope to the last mounted .game so DOM accumulation from earlier tests
    // does not pollute this assertion.
    let document = leptos::prelude::document();
    let games = document
        .query_selector_all(".game")
        .expect("query_selector_all");
    let last_game = games
        .item(games.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("last .game is an Element");

    assert!(
        last_game
            .query_selector(".board-main .map")
            .expect("query ok")
            .is_some(),
        ".map must be a descendant of .board-main"
    );
    assert!(
        last_game
            .query_selector(".board-main .investigators")
            .expect("query ok")
            .is_some(),
        ".investigators must be a descendant of .board-main"
    );
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_as_card_in_threat_area() {
    use game_core::state::InvestigatorId;
    use game_core::test_support::fixtures::{test_enemy, test_investigator};

    let mut enemy = test_enemy(1, "Ghoul Priest");
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();

    let html = render_state(state).await;

    let games = leptos::prelude::document()
        .query_selector_all(".game")
        .expect("query_selector_all");
    let last_game = games
        .item(games.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("last .game is an Element");
    let card = last_game
        .query_selector(".threat .card-row .card")
        .expect("query_selector");
    assert!(
        card.is_some(),
        "engaged enemy should render as a card: {html}"
    );
    assert!(html.contains("Ghoul Priest"), "enemy name missing: {html}");
}

#[wasm_bindgen_test]
async fn threat_area_treachery_renders_as_card() {
    use game_core::state::{CardCode, CardInPlay, CardInstanceId};

    let mut inv = test_investigator(1);
    inv.threat_area = vec![CardInPlay::enter_play(
        CardCode::new("_synth_treachery"),
        CardInstanceId(0),
    )];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;

    let games = leptos::prelude::document()
        .query_selector_all(".game")
        .expect("query_selector_all");
    let last_game = games
        .item(games.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("last .game is an Element");
    let card = last_game
        .query_selector(".threat .card-row .card")
        .expect("query_selector");
    assert!(
        card.is_some(),
        "threat-area treachery should render as a card: {html}"
    );
}

#[wasm_bindgen_test]
async fn board_renders_a_new_game_button() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    leptos::task::tick().await;

    // Scope to the last mounted .board (DOM accumulates across tests).
    let boards = leptos::prelude::document()
        .query_selector_all(".board")
        .expect("query_selector_all");
    let last = boards
        .item(boards.length() - 1)
        .expect("at least one .board section")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    assert!(
        last.query_selector(".new-game").expect("query").is_some(),
        "BoardView must render a .new-game button"
    );
}
