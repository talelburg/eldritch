//! Headless browser test for `PickerView` (#459): clicking "Create game"
//! sends a `CreateGameRequest` with the picked investigator's roster on the
//! `CreateTx` channel — Roland by default, Agnes once her radio is selected.
#![cfg(target_arch = "wasm32")]
use futures::channel::mpsc;
use futures::StreamExt as _;
use leptos::prelude::*;
use protocol::CreateGameRequest;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::picker::{CreateTx, PickerView};
use web::store::{ClientState, ConnStatus};
use web_sys::HtmlElement;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount a fresh picker awaiting a roster; returns the receiving end of its
/// `CreateTx`.
fn mount_picker() -> mpsc::UnboundedReceiver<CreateGameRequest> {
    let store = RwSignal::new(ClientState {
        status: ConnStatus::AwaitingRoster,
        ..Default::default()
    });
    let (tx, rx) = mpsc::unbounded::<CreateGameRequest>();
    mount_to_body(move || {
        provide_context(store);
        provide_context::<CreateTx>(tx.clone());
        view! { <PickerView/> }
    });
    rx
}

/// Click the last element in the document matching `selector` (each test
/// mounts its own picker into the shared body, so the last one is this test's).
fn click_last(selector: &str) {
    let doc = web_sys::window().unwrap().document().unwrap();
    let all = doc.query_selector_all(selector).unwrap();
    assert!(all.length() > 0, "nothing matches {selector}");
    all.item(all.length() - 1)
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap()
        .click();
}

#[wasm_bindgen_test]
async fn create_button_sends_a_roster() {
    let mut rx = mount_picker();
    click_last(".create-game");

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.scenario_id, "the-gathering");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01001");
    assert!(
        !req.roster[0].deck.is_empty(),
        "Roland is seated with the default deck"
    );
}

#[wasm_bindgen_test]
async fn selecting_the_daisy_radio_sends_daisy_roster() {
    let mut rx = mount_picker();
    click_last("input[name=inv][value='01002']");
    click_last(".create-game");

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01002");
    assert!(
        req.roster[0].deck.iter().any(|c| c.as_str() == "01035"),
        "Daisy is seated with her scaffold deck (Medical Texts 01035 in it)"
    );
}

#[wasm_bindgen_test]
async fn selecting_the_skids_radio_sends_skids_roster() {
    let mut rx = mount_picker();
    click_last("input[name=inv][value='01003']");
    click_last(".create-game");

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01003");
    assert!(
        req.roster[0].deck.iter().any(|c| c.as_str() == "01016"),
        "Skids is seated with his scaffold deck (.45 Automatic 01016 in it)"
    );
}

#[wasm_bindgen_test]
async fn selecting_the_agnes_radio_sends_agnes_roster() {
    let mut rx = mount_picker();
    click_last("input[name=inv][value='01004']");
    click_last(".create-game");

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01004");
    assert!(
        req.roster[0].deck.iter().any(|c| c.as_str() == "01059"),
        "Agnes is seated with her scaffold deck (Holy Rosary 01059 in it)"
    );
}

#[wasm_bindgen_test]
async fn selecting_the_wendy_radio_sends_wendy_roster() {
    let mut rx = mount_picker();
    click_last("input[name=inv][value='01005']");
    click_last(".create-game");

    let req = rx.next().await.expect("a CreateGameRequest was sent");
    assert_eq!(req.roster.len(), 1);
    assert_eq!(req.roster[0].investigator.as_str(), "01005");
    assert!(
        req.roster[0].deck.iter().any(|c| c.as_str() == "01014"),
        "Wendy is seated with her scaffold deck (Wendy's Amulet 01014 in it)"
    );
}
