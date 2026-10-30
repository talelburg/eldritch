//! Headless render tests for the app status bar (status / rejection / waiting
//! on / New game), which lives in the page header.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{ChoiceOption, EngineOutcome, InputRequest, OptionId, Responder};
use game_core::state::{GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{self, seat, SeatLookup as _};
use leptos::prelude::{document, provide_context, RwSignal, Update};
use leptos::{mount, task};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::status_bar::StatusBarView;
use web::store::{ClientState, ConnStatus};
use web_sys::Element;

/// Mount `StatusBarView` against a fresh store; returns the store so a test can
/// drive `status`/`last_rejection`.
fn mount() -> RwSignal<ClientState> {
    let store = RwSignal::new(ClientState::default());
    mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <StatusBarView/> }
    });
    store
}

#[wasm_bindgen_test]
async fn version_mismatch_status_renders_actionable_message() {
    let store = mount();
    store.update(|s| s.status = ConnStatus::VersionMismatch);
    task::tick().await;

    // Scope to the last mounted .status line (DOM accumulates across tests).
    let lines = document()
        .query_selector_all(".status")
        .expect("query_selector_all");
    let html = lines
        .item(lines.length() - 1)
        .expect("at least one .status line")
        .dyn_ref::<Element>()
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
async fn status_bar_renders_a_new_game_button() {
    let _store = mount();
    task::tick().await;

    // Scope to the last mounted .status-bar (DOM accumulates across tests).
    let bars = document()
        .query_selector_all(".status-bar")
        .expect("query_selector_all");
    let last = bars
        .item(bars.length() - 1)
        .expect("at least one .status-bar")
        .dyn_into::<Element>()
        .expect("Element");
    assert!(
        last.query_selector(".new-game").expect("query").is_some(),
        "status bar must render a .new-game button"
    );
}

const FIRST: InvestigatorId = InvestigatorId(1);
const SECOND: InvestigatorId = InvestigatorId(2);

/// Mount `StatusBarView` over a two-seat game resting at `outcome`, and return
/// the text of its "waiting on" line, if it renders one.
async fn waiting_on(outcome: Option<EngineOutcome>) -> Option<String> {
    let store = mount();
    store.update(|s| {
        s.game = Some(two_seats());
        s.outcome = outcome;
    });
    task::tick().await;
    let bars = document()
        .query_selector_all(".status-bar")
        .expect("query_selector_all");
    bars.item(bars.length() - 1)
        .expect("at least one .status-bar")
        .dyn_into::<Element>()
        .expect("Element")
        .query_selector(".waiting-on")
        .expect("query")
        .map(|line| line.text_content().unwrap_or_default())
}

/// The two-seat game [`waiting_on`] mounts.
fn two_seats() -> GameState {
    GameStateBuilder::new()
        .with_seat(seat(FIRST.0))
        .with_seat(seat(SECOND.0))
        .with_seating([FIRST, SECOND])
        .build()
}

/// The name `id` is shown by in [`two_seats`].
fn name(id: InvestigatorId) -> String {
    two_seats().name_of(id)
}

#[wasm_bindgen_test]
async fn the_status_bar_names_the_investigator_a_prompt_waits_on() {
    let line = waiting_on(Some(test_support::awaiting_confirm_input_for(
        "Draw an encounter card",
        SECOND,
    )))
    .await
    .expect("a prompt shows who it waits on");
    assert!(line.contains(&name(SECOND)), "{line}");
    assert!(!line.contains(&name(FIRST)), "{line}");
}

#[wasm_bindgen_test]
async fn the_status_bar_says_when_the_lead_is_deciding() {
    let request = InputRequest::pick_single("Choose whom the enemy engages", vec![])
        .answered_by(Responder::Lead(SECOND));
    let line = waiting_on(Some(test_support::awaiting_request(request)))
        .await
        .expect("a prompt shows who it waits on");
    assert!(line.contains(&name(SECOND)), "{line}");
    assert!(line.contains("lead"), "{line}");
}

#[wasm_bindgen_test]
async fn a_window_several_answer_waits_on_each_investigator_offered_an_option() {
    let request = InputRequest::pick_single(
        "Resolution window",
        vec![
            ChoiceOption::new(OptionId(0), "Resolve").offered_to(FIRST),
            ChoiceOption::new(OptionId(1), "Resolve").offered_to(SECOND),
        ],
    )
    .skippable()
    .answered_by(Responder::Several);
    let line = waiting_on(Some(test_support::awaiting_request(request)))
        .await
        .expect("a window shows who it waits on");
    assert!(line.contains(&name(FIRST)), "{line}");
    assert!(line.contains(&name(SECOND)), "{line}");
}

#[wasm_bindgen_test]
async fn with_no_prompt_the_status_bar_waits_on_nobody() {
    assert_eq!(waiting_on(None).await, None);
}
