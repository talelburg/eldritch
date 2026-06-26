//! Headless browser test for `PickerView` (#459): clicking "Create game"
//! sends a `CreateGameRequest` with Roland's roster on the `CreateTx` channel.
#![cfg(target_arch = "wasm32")]
use futures::channel::mpsc;
use futures::StreamExt as _;
use leptos::prelude::*;
use protocol::CreateGameRequest;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::picker::{CreateTx, PickerView};
use web::store::{ClientState, ConnStatus};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn create_button_sends_a_roster() {
    let store = RwSignal::new(ClientState {
        status: ConnStatus::AwaitingRoster,
        ..Default::default()
    });
    let (tx, mut rx) = mpsc::unbounded::<CreateGameRequest>();
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<CreateTx>(tx.clone());
        view! { <PickerView/> }
    });
    // Click the create button.
    let doc = web_sys::window().unwrap().document().unwrap();
    let btn = doc
        .query_selector(".create-game")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap();
    btn.click();

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.scenario_id, "the-gathering");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01001");
    assert!(
        !req.roster[0].deck.is_empty(),
        "Roland is seated with the default deck"
    );
}
