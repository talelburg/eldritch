//! Helpers for a ⋯ menu, wherever it sits: the lobby header's, the board's,
//! or a "Your games" row's. Each names its menu by a selector `within` it
//! sits, e.g. `.lobby-header` or `.board-menu`.
#![cfg(target_arch = "wasm32")]

use leptos::task;
use wasm_bindgen::JsCast as _;
use web_sys::HtmlElement;

use crate::common::BoardHandle;
use crate::lobby::{click, texts};

/// Open the ⋯ menu `within`; return its entries' labels, in document order,
/// submenus' included.
pub(crate) async fn open_menu(handle: &BoardHandle, within: &str) -> Vec<String> {
    let items = format!("{within} .more-menu .menu-item");
    assert!(texts(handle, &items).is_empty(), "the menu starts closed");
    click(handle, &format!("{within} .more-menu-toggle"));
    task::tick().await;
    texts(handle, &items)
}

/// Choose the entry labelled `label` from the open ⋯ menu `within`.
pub(crate) async fn choose(handle: &BoardHandle, within: &str, label: &str) {
    let nodes = handle
        .root
        .query_selector_all(&format!("{within} .more-menu .menu-item"))
        .expect("query");
    let entry = (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .find(|node| node.text_content().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("the menu has {label}"));
    entry
        .dyn_ref::<HtmlElement>()
        .expect("an HTML element")
        .click();
    task::tick().await;
}
