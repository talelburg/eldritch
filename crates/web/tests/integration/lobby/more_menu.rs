//! The lobby header's ⋯ menu: Leave for a member, Remove `<member>` for the
//! creator, each sending its command.

use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen_test::*;

use super::{click, mount_lobby, sent, tals_lobby_with_sam, texts, SAM};
use crate::common::{BoardHandle, YOU};

/// Open the header's ⋯ menu; return its entries' labels.
async fn open_menu(handle: &BoardHandle) -> Vec<String> {
    assert!(
        texts(handle, ".lobby-header .more-menu .menu-item").is_empty(),
        "the menu starts closed"
    );
    click(handle, ".lobby-header .more-menu-toggle");
    task::tick().await;
    texts(handle, ".lobby-header .more-menu .menu-item")
}

/// Choose the entry labelled `label` from the open menu.
async fn choose(handle: &BoardHandle, label: &str) {
    let nodes = handle
        .root
        .query_selector_all(".lobby-header .more-menu .menu-item")
        .expect("query");
    let index = (0..nodes.length())
        .find(|&i| {
            nodes
                .item(i)
                .and_then(|node| node.text_content())
                .as_deref()
                == Some(label)
        })
        .unwrap_or_else(|| panic!("the menu has {label}"));
    let entry = nodes.item(index).expect("the entry");
    wasm_bindgen::JsCast::dyn_ref::<web_sys::HtmlElement>(&entry)
        .expect("an HTML element")
        .click();
    task::tick().await;
}

#[wasm_bindgen_test]
async fn the_creators_menu_removes_each_other_member_and_has_no_leave() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    assert_eq!(open_menu(&handle).await, ["Remove Sam"]);
}

#[wasm_bindgen_test]
async fn a_members_menu_has_leave_only() {
    let handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;
    assert_eq!(open_menu(&handle).await, ["Leave"]);
}

#[wasm_bindgen_test]
async fn leave_sends_leave_and_closes_the_menu() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;
    open_menu(&handle).await;

    choose(&handle, "Leave").await;

    assert!(matches!(sent(&mut handle), ClientMessage::Leave));
    assert!(
        texts(&handle, ".lobby-header .more-menu .menu-item").is_empty(),
        "choosing an entry closes the menu"
    );
}

#[wasm_bindgen_test]
async fn remove_sends_remove_of_that_member() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    open_menu(&handle).await;

    choose(&handle, "Remove Sam").await;

    assert!(matches!(
        sent(&mut handle),
        ClientMessage::Remove { member } if member == SAM
    ));
}
