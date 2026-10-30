//! Shared helpers for the web crate's browser tests.

use leptos::prelude::document;
use wasm_bindgen::JsCast as _;
use web_sys::Element;

/// The last element matching `sel`: the browser tests share one page and
/// nothing unmounts, so a test reads the last match.
pub(crate) fn last_mounted(sel: &str) -> Element {
    let nodes = document()
        .query_selector_all(sel)
        .expect("query_selector_all");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .unwrap_or_else(|| panic!("at least one {sel}"))
}
