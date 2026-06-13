//! Headless render tests for `BoardView` (P6.5). Feed a constructed
//! `GameState` through the store and assert on the rendered DOM.
//! wasm32-only (browser DOM); native jobs skip this file.
#![cfg(target_arch = "wasm32")]

use game_core::state::GameStateBuilder;
use game_core::state::{Act, Agenda, CardCode, Phase};
use game_core::test_support::fixtures::{test_enemy, test_investigator, test_location};
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
        round_end_advance: None,
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
async fn locations_panel_renders_name_shroud_clues_revealed() {
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
    assert!(html.contains("revealed"), "revealed flag missing: {html}");
}

#[wasm_bindgen_test]
async fn investigators_panel_renders_stats_and_hand() {
    use game_core::state::{CardCode, CardInPlay, CardInstanceId};

    let mut inv = test_investigator(1);
    inv.name = "Roland Banks".to_string();
    inv.damage = 2; // 2/8
    inv.horror = 1; // 1/8
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
}

#[wasm_bindgen_test]
async fn enemies_panel_renders_name_stats_engagement() {
    use game_core::state::InvestigatorId;

    let mut enemy = test_enemy(4, "Swarm of Rats");
    enemy.damage = 1; // 1/2
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();

    let html = render_state(state).await;

    assert!(html.contains("Swarm of Rats"), "enemy name missing: {html}");
    assert!(html.contains("fight 2"), "fight missing: {html}");
    assert!(html.contains("evade 2"), "evade missing: {html}");
    assert!(html.contains("1/2"), "enemy health missing: {html}");
    assert!(
        html.contains("engaged with 1"),
        "engagement missing: {html}"
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
