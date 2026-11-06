//! Headless render test for `EventLogView`: seed the store with one
//! `LogBatch` and assert the panel renders the batch header and an event's
//! `Debug` text.
//!
//! wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::engine::{ChoiceOption, EngineOutcome, InputRequest, OptionId, Responder};
use game_core::event::Event;
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::event_log::EventLogView;
use web::store::{self, ClientState, LogBatch};
use web_sys::{Element, HtmlElement};

use crate::common;

#[wasm_bindgen_test]
async fn renders_batches_with_headers_and_event_debug() {
    let store = RwSignal::new(ClientState::default());
    mount_to_body(move || {
        provide_context(store);
        leptos::view! { <EventLogView/> }
    });
    store.update(|s| {
        s.log.push(LogBatch {
            header: "Move to Cellar".into(),
            events: vec![Event::ScenarioStarted],
        });
    });
    task::tick().await;

    let logs = document().query_selector_all(".event-log").expect("query");
    assert!(logs.length() >= 1, "no .event-log element rendered");
    let panel = logs
        .item(logs.length() - 1)
        .expect("an .event-log panel")
        .dyn_into::<Element>()
        .expect("Element");
    let text = panel.text_content().unwrap_or_default();
    assert!(text.contains("Move to Cellar"), "header rendered: {text}");
    assert!(
        text.contains("ScenarioStarted"),
        "event Debug rendered: {text}"
    );
}

#[wasm_bindgen_test]
async fn log_drawer_starts_closed_and_opens_from_its_toggle() {
    let store = RwSignal::new(ClientState::default());
    mount_to_body(move || {
        provide_context(store);
        leptos::view! { <EventLogView/> }
    });
    task::tick().await;

    let logs = document()
        .query_selector_all(".event-log")
        .expect("query ok");
    let panel = logs
        .item(logs.length() - 1)
        .expect("an .event-log panel")
        .dyn_into::<Element>()
        .expect("Element");

    let scroll = panel
        .query_selector(".log-scroll")
        .expect("query ok")
        .expect(".log-scroll present");
    let open = |el: &Element| el.class_list().contains("open");
    let hidden = |el: &Element| el.class_list().contains("hidden");
    assert!(!open(&panel), "the drawer starts closed");
    assert!(hidden(&scroll), "a closed drawer hides its body");

    let toggle = panel
        .query_selector(".log-toggle")
        .expect("query ok")
        .expect(".log-toggle present")
        .dyn_into::<HtmlElement>()
        .expect("HtmlElement");
    toggle.click();
    task::tick().await;
    assert!(open(&panel), "the toggle opens the drawer");
    assert!(!hidden(&scroll), "an open drawer shows its body");

    toggle.click();
    task::tick().await;
    assert!(!open(&panel), "the toggle closes it again");
    assert!(hidden(&scroll), "and hides its body");
}

/// The event log's text after a socket hands the store a `Hello` from
/// `build` carrying setup's events.
async fn log_after_a_hello_from(build: &str) -> String {
    let (store, container) = common::mount(|| leptos::view! { <EventLogView/> });
    let frame = common::hello_frame(
        build,
        GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .build(),
        EngineOutcome::Done,
        vec![Event::ScenarioStarted],
    );
    store.update(|s| {
        store::receive(s, &frame);
    });
    task::tick().await;
    container.text_content().unwrap_or_default()
}

#[wasm_bindgen_test]
async fn a_hello_from_another_build_logs_none_of_its_setup() {
    let text = log_after_a_hello_from(&common::another_build()).await;
    assert!(
        !text.contains("ScenarioStarted"),
        "a mismatched Hello reaches the overlay, not the log: {text}"
    );
}

/// `el`'s stacking level among the page's fixed layers: its computed
/// `z-index`, or `None` when it isn't positioned, and so sits under every
/// layer that is.
fn stacking(el: &Element) -> Option<i32> {
    if common::computed(el, "position") == "static" {
        return None;
    }
    common::computed(el, "z-index").parse().ok()
}

/// During "A Lapse in Time" the `▸ log` toggle sat under the decision modal's
/// scrim and couldn't be opened. The log, closed to its toggle or open as a
/// drawer, stacks above the scrim. Measured as stacking levels: the page is
/// shared, so another test's overlay may lie over any point a hit test reads.
#[wasm_bindgen_test]
async fn the_log_and_its_toggle_sit_above_a_decisions_scrim() {
    common::inject_style();
    let (_store, container) = common::mount(|| leptos::view! { <EventLogView/> });
    let decision = test_support::awaiting_request(
        InputRequest::pick_single(
            "Choose one",
            vec![
                ChoiceOption::new(OptionId(0), "Each investigator discards 1 card at random"),
                ChoiceOption::new(OptionId(1), "The lead investigator takes 2 horror"),
            ],
        )
        .deciding()
        .answered_by(Responder::Investigator(InvestigatorId(1))),
    );
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let board = common::mount_board(
        common::hello(state, decision),
        &test_support::TEST_REGISTRY,
    )
    .await;
    let scrim = board
        .root
        .query_selector(".decision-backdrop")
        .expect("query")
        .expect("the decision modal's scrim");
    let scrim_level = stacking(&scrim).expect("the scrim is positioned");

    let log = container
        .query_selector(".event-log")
        .expect("query")
        .expect("the event log");
    assert!(
        stacking(&log).is_some_and(|level| level > scrim_level),
        "the closed log's toggle sits above the scrim ({scrim_level})"
    );
    log.query_selector(".log-toggle")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("the toggle")
        .click();
    task::tick().await;
    assert!(log.class_list().contains("open"), "the toggle opens it");
    assert!(
        stacking(&log).is_some_and(|level| level > scrim_level),
        "the open drawer sits above the scrim ({scrim_level})"
    );
}

#[wasm_bindgen_test]
async fn a_hello_from_this_build_logs_its_setup() {
    let text = log_after_a_hello_from(protocol::build_id()).await;
    assert!(text.contains("Setup"), "setup batch header: {text}");
    assert!(text.contains("ScenarioStarted"), "setup event: {text}");
}
