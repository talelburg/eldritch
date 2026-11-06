//! The lobby header's ⋯ menu: Leave for a member, Remove `<member>` for the
//! creator, each sending its command.

use protocol::ClientMessage;
use wasm_bindgen_test::*;

use super::{mount_lobby, sent, tals_lobby_with_sam, texts, HEADER, SAM};
use crate::common::YOU;
use crate::more_menu::{choose, open_menu};

#[wasm_bindgen_test]
async fn the_creators_menu_removes_each_other_member_and_deletes_and_has_no_leave() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    assert_eq!(open_menu(&handle, HEADER).await, ["Remove Sam", "Delete"]);
}

#[wasm_bindgen_test]
async fn a_members_menu_has_leave_only() {
    let handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;
    assert_eq!(open_menu(&handle, HEADER).await, ["Leave"]);
}

#[wasm_bindgen_test]
async fn leave_sends_leave_and_closes_the_menu() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;
    open_menu(&handle, HEADER).await;

    choose(&handle, HEADER, "Leave").await;

    assert!(matches!(sent(&mut handle), ClientMessage::Leave));
    assert!(
        texts(&handle, ".lobby-header .more-menu .menu-item").is_empty(),
        "choosing an entry closes the menu"
    );
}

#[wasm_bindgen_test]
async fn remove_sends_remove_of_that_member() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    open_menu(&handle, HEADER).await;

    choose(&handle, HEADER, "Remove Sam").await;

    assert!(matches!(
        sent(&mut handle),
        ClientMessage::Remove { member } if member == SAM
    ));
}
