//! Headless browser test for `PickerView`: clicking "Create game" sends a
//! `CreateGameRequest` with the picked investigator's default-deck roster on
//! the `CreateTx` channel — with the deck's fixed weakness by default, or a
//! random basic weakness once the checkbox is checked.
//!
//! These are the rosters
//! `crates/scenarios/tests/the_gathering_default_decks.rs` plays and the
//! server's registries test creates.
#![cfg(target_arch = "wasm32")]
use cards::decks::{self, WeaknessChoice};
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

/// Mount a fresh picker awaiting a roster; returns its store and the receiving
/// end of its `CreateTx`.
fn mount_picker() -> (
    RwSignal<ClientState>,
    mpsc::UnboundedReceiver<CreateGameRequest>,
) {
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
    (store, rx)
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

/// The checkbox choosing a random basic weakness over the fixed one.
const RANDOM_WEAKNESS: &str = "input.random-basic-weakness";

/// Click "Create game" and receive the request it sends.
async fn create(rx: &mut mpsc::UnboundedReceiver<CreateGameRequest>) -> CreateGameRequest {
    click_last(".create-game");
    rx.next().await.expect("a CreateGameRequest was sent")
}

#[wasm_bindgen_test]
async fn create_button_sends_rolands_fixed_roster_by_default() {
    let (_, mut rx) = mount_picker();
    let req = create(&mut rx).await;

    assert_eq!(req.scenario_id, "the-gathering");
    assert_eq!(
        req.roster,
        vec![decks::ROLAND.roster_entry(WeaknessChoice::Fixed)]
    );
}

#[wasm_bindgen_test]
async fn each_radio_sends_its_investigators_fixed_roster() {
    let (_, mut rx) = mount_picker();
    for deck in decks::DEFAULT_DECKS {
        click_last(&format!("input[name=inv][value='{}']", deck.investigator));
        let req = create(&mut rx).await;
        assert_eq!(
            req.roster,
            vec![deck.roster_entry(WeaknessChoice::Fixed)],
            "{}",
            deck.investigator
        );
    }
}

/// Checked, the box sends the random roster: the fixed weakness is gone from
/// the deck, and seating is asked to draw one.
#[wasm_bindgen_test]
async fn checking_random_basic_weakness_sends_the_random_roster() {
    let (_, mut rx) = mount_picker();
    click_last("input[name=inv][value='01003']");
    click_last(RANDOM_WEAKNESS);
    let req = create(&mut rx).await;

    let entry = &req.roster[0];
    assert_eq!(entry, &decks::SKIDS.roster_entry(WeaknessChoice::Random));
    assert!(!entry
        .deck
        .iter()
        .any(|c| c.as_str() == decks::SKIDS.weakness));
    assert_eq!(entry.random_basic_weaknesses, 1);
}

/// A store update re-renders the overlay; the checkbox and the picked radio
/// both survive it.
#[wasm_bindgen_test]
async fn the_checkbox_and_the_pick_survive_a_re_render() {
    let (store, mut rx) = mount_picker();
    click_last("input[name=inv][value='01004']");
    click_last(RANDOM_WEAKNESS);
    store.update(|s| s.last_rejection = Some("re-render".to_string()));

    let req = create(&mut rx).await;
    assert_eq!(
        req.roster,
        vec![decks::AGNES.roster_entry(WeaknessChoice::Random)]
    );
}
