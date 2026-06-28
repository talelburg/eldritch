//! Headless render test for `EventLogView` (#505): seed the store with a couple
//! of `LogBatch`es and assert the panel renders each header and its events as
//! Debug text, oldest-first. wasm32-only (browser DOM).
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
