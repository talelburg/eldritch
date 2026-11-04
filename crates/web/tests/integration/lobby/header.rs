//! The lobby's header: the members by display name, and the invite link with
//! its Copy button.

use wasm_bindgen_test::*;

use super::{mount_lobby, one, tals_lobby_with_sam, texts, GAME_ID, SAM};
use crate::common::YOU;

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
