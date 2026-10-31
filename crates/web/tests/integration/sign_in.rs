//! Signed out, the app shows the sign-in screen with the methods the server
//! listed; signed in, the status bar carries a sign-out control.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use leptos::task;
use protocol::{SignInMethod, SignedIn};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::sign_in::SignInView;
use web::status_bar::StatusBarView;
use web::store::{ClientState, ConnStatus};
use web_sys::HtmlElement;

/// Mount `view` into a container of its own over a fresh store; return the
/// store and the container. The DOM accumulates across tests in one browser
/// session, so every assertion is scoped to this container.
fn mount<V: IntoView + 'static>(
    view: impl FnOnce() -> V + 'static,
) -> (RwSignal<ClientState>, HtmlElement) {
    let container = document()
        .create_element("div")
        .expect("create_element")
        .dyn_into::<HtmlElement>()
        .expect("HtmlElement");
    document()
        .body()
        .expect("body")
        .append_child(&container)
        .expect("append_child");

    let store = RwSignal::new(ClientState::default());
    mount_to(container.clone(), move || {
        provide_context(store);
        view()
    })
    .forget();
    (store, container)
}

fn dev_picker() -> SignInMethod {
    SignInMethod {
        label: "Dev sign-in".to_string(),
        url: "/auth/dev".to_string(),
    }
}

#[wasm_bindgen_test]
async fn signed_out_the_screen_links_each_listed_method() {
    let (store, container) = mount(|| view! { <SignInView/> });
    store.update(|s| s.status = ConnStatus::SignedOut(vec![dev_picker()]));
    task::tick().await;

    let screen = container
        .query_selector(".sign-in")
        .expect("query")
        .expect("a sign-in screen");
    let links = screen
        .query_selector_all("a.sign-in-method")
        .expect("query");
    assert_eq!(links.length(), 1, "{}", screen.inner_html());
    let link = links
        .item(0)
        .expect("a link")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    assert_eq!(link.get_attribute("href").as_deref(), Some("/auth/dev"));
    assert_eq!(link.text_content().as_deref(), Some("Dev sign-in"));
    assert!(
        screen
            .query_selector(".no-sign-in")
            .expect("query")
            .is_none(),
        "a listed method means sign-in is available"
    );
}

#[wasm_bindgen_test]
async fn signed_out_with_no_method_the_screen_says_none_is_available() {
    let (store, container) = mount(|| view! { <SignInView/> });
    store.update(|s| s.status = ConnStatus::SignedOut(vec![]));
    task::tick().await;

    let note = container
        .query_selector(".sign-in .no-sign-in")
        .expect("query")
        .expect("the no-sign-in note");
    assert!(
        note.text_content()
            .unwrap_or_default()
            .contains("No sign-in is available"),
        "{:?}",
        note.text_content()
    );
    assert!(container
        .query_selector("a.sign-in-method")
        .expect("query")
        .is_none());
}

#[wasm_bindgen_test]
async fn the_sign_in_screen_is_absent_while_signed_in() {
    for status in [
        ConnStatus::Connecting,
        ConnStatus::Connected,
        ConnStatus::Reconnecting,
        ConnStatus::Failed,
        ConnStatus::AwaitingRoster,
        ConnStatus::VersionMismatch,
    ] {
        let (store, container) = mount(|| view! { <SignInView/> });
        store.update(|s| s.status = status.clone());
        task::tick().await;

        assert!(
            container
                .query_selector(".sign-in")
                .expect("query")
                .is_none(),
            "{status:?} shows no sign-in screen"
        );
    }
}

#[wasm_bindgen_test]
async fn signed_in_the_status_bar_offers_sign_out() {
    let (store, container) = mount(|| view! { <StatusBarView/> });
    store.update(|s| {
        s.status = ConnStatus::Connected;
        s.account = Some(SignedIn {
            account_id: 1,
            display_name: "Sam".to_string(),
        });
    });
    task::tick().await;

    assert!(
        container
            .query_selector(".status-bar .sign-out")
            .expect("query")
            .is_some(),
        "a signed-in status bar carries a sign-out control"
    );
    let account = container
        .query_selector(".status-bar .account")
        .expect("query")
        .expect("the signed-in account is named");
    assert!(account.text_content().unwrap_or_default().contains("Sam"));
}

#[wasm_bindgen_test]
async fn signed_out_the_status_bar_offers_no_sign_out() {
    let (store, container) = mount(|| view! { <StatusBarView/> });
    store.update(|s| s.status = ConnStatus::SignedOut(vec![dev_picker()]));
    task::tick().await;

    assert!(container
        .query_selector(".status-bar .sign-out")
        .expect("query")
        .is_none());
}
