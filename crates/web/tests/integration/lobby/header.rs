//! The lobby's header: the members by display name, and the invite link with
//! its Copy button.

use leptos::prelude::*;
use wasm_bindgen_test::*;
use web::store::ConnStatus;

use super::{mount_lobby, one, tals_lobby_with_sam, texts, GAME_ID, SAM};
use crate::common::{self, YOU};

#[wasm_bindgen_test]
async fn the_header_lists_the_members_by_display_name() {
    let handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;
    assert_eq!(texts(&handle, ".lobby-member"), ["Tal", "Sam"]);
}

#[wasm_bindgen_test]
async fn the_header_shows_the_games_link_with_a_copy_button() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;

    let link = one(&handle, ".invite-link")
        .text_content()
        .unwrap_or_default();
    assert!(
        link.ends_with(&format!("/games/{GAME_ID}")) && link.starts_with("http"),
        "the link is this game's address under the page's origin: {link}"
    );
    assert_eq!(
        one(&handle, "button.copy-link").text_content().as_deref(),
        Some("Copy")
    );
}

/// The lobby takes clicks while the board behind it is locked: the lobby
/// status locks the board, and the lobby sits outside the lockout wrapper.
#[wasm_bindgen_test]
async fn the_lobby_stays_clickable_over_the_locked_board() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    assert_eq!(
        handle.store.with_untracked(|s| s.status.clone()),
        ConnStatus::InLobby
    );
    assert!(handle.lock().has_attribute("inert"));
    common::assert_live(&one(&handle, ".lobby"), "the lobby");
}
