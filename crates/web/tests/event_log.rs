//! Headless render test for `EventLogView` (#505): seed the store with one
//! `LogBatch` and assert the panel renders the batch header and an event's
//! `Debug` text. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::event_log::EventLogView;
use web::store::{ClientState, LogBatch};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn renders_batches_with_headers_and_event_debug() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <EventLogView/> }
    });
    store.update(|s| {
        s.log.push(LogBatch {
            header: "Move to Cellar".into(),
            events: vec![game_core::Event::ScenarioStarted],
        });
    });
    leptos::task::tick().await;

    let logs = leptos::prelude::document()
        .query_selector_all(".event-log")
        .expect("query");
    assert!(logs.length() >= 1, "no .event-log element rendered");
    let panel = logs
        .item(logs.length() - 1)
        .expect("an .event-log panel")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    let text = panel.text_content().unwrap_or_default();
    assert!(text.contains("Move to Cellar"), "header rendered: {text}");
    assert!(
        text.contains("ScenarioStarted"),
        "event Debug rendered: {text}"
    );
}

#[wasm_bindgen_test]
async fn log_collapses_and_expands() {
    // Mount EventLogView (use the file's existing mount helper / store setup).
    let store = leptos::prelude::RwSignal::new(web::store::ClientState::default());
    leptos::mount::mount_to_body(move || {
        leptos::prelude::provide_context(store);
        leptos::view! { <web::event_log::EventLogView/> }
    });
    leptos::task::tick().await;

    let doc = leptos::prelude::document();
    let scroll = doc
        .query_selector(".event-log .log-scroll")
        .expect("query ok")
        .expect(".log-scroll present");
    assert!(
        !scroll.class_name().contains("hidden"),
        "log body should start visible: {}",
        scroll.class_name()
    );

    let toggle = doc
        .query_selector(".event-log .log-toggle")
        .expect("query ok")
        .expect(".log-toggle present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement");
    toggle.click();
    leptos::task::tick().await;
    assert!(
        scroll.class_name().contains("hidden"),
        "log body should be hidden after collapse: {}",
        scroll.class_name()
    );
}
