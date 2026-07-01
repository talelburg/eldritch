//! Headless tests for `ContextMenu` (interactivity S1, #536): an open menu
//! renders one button per option; clicking one submits the matching
//! `ResolveInput(PickSingle)` and closes the menu; a closed menu renders nothing.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::LocationId;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::ContextMenu;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount a `ContextMenu` with one `Location(10)`-anchored option, a fresh store
/// + outbound channel, and the given initial `open` state. Returns the `open`
/// signal and the receiver for submitted frames.
async fn mount(open_initial: bool) -> (RwSignal<bool>, mpsc::UnboundedReceiver<ClientMessage>) {
    let store = RwSignal::new(ClientState::default());
    let open = RwSignal::new(open_initial);
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    let options = vec![ChoiceOption::new(
        OptionId(0),
        "Investigate",
        OptionTarget::Location(LocationId(10)),
    )];
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <div class="tc-root"><ContextMenu options=options.clone() open=open/></div> }
    });
    leptos::task::tick().await;
    (open, rx)
}

/// The `.menu-item` buttons in the LAST-mounted `.tc-root` — scoped to this
/// test's wrapper so DOM accumulation across tests (one page) can't let a prior
/// test's open menu shadow a "renders nothing" assertion.
fn menu_items() -> web_sys::NodeList {
    let roots = document().query_selector_all(".tc-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .tc-root wrapper")
        .query_selector_all(".menu-item")
        .expect("query")
}

#[wasm_bindgen_test]
async fn open_menu_renders_a_button_per_option() {
    let _ = mount(true).await;
    let items = menu_items();
    assert_eq!(items.length(), 1);
    assert_eq!(
        items
            .item(0)
            .and_then(|n| n.text_content())
            .unwrap_or_default(),
        "Investigate"
    );
}

#[wasm_bindgen_test]
async fn closed_menu_renders_no_context_menu() {
    let _ = mount(false).await;
    // The just-mounted (closed) menu rendered no `.context-menu`, so its subtree
    // has no items.
    assert_eq!(menu_items().length(), 0);
}

#[wasm_bindgen_test]
async fn clicking_an_item_submits_pick_single_and_closes() {
    let (open, mut rx) = mount(true).await;
    let items = menu_items();
    items
        .item(0)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;

    let msg = rx.try_recv().expect("a frame was sent after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
    assert!(!open.get_untracked(), "menu closes after a selection");
}
