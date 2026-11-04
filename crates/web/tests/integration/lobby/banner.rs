//! The banner an account sees on the first opening of a game's link, which is
//! what made it a member: there is no Join button to press.

use wasm_bindgen_test::*;

use super::{mount_lobby, one, tals_lobby_with_sam, SAM};
use crate::common::YOU;

#[wasm_bindgen_test]
async fn opening_the_link_shows_whose_game_you_joined() {
    let handle = mount_lobby(tals_lobby_with_sam(), SAM, true).await;
    let banner = one(&handle, ".joined-banner")
        .text_content()
        .unwrap_or_default();
    assert!(banner.contains("Tal's game"), "{banner}");
    assert!(
        handle
            .root
            .query_selector(".join")
            .expect("query")
            .is_none(),
        "no Join button"
    );
}

#[wasm_bindgen_test]
async fn a_member_who_was_already_one_sees_no_banner() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    assert!(handle
        .root
        .query_selector(".joined-banner")
        .expect("query")
        .is_none());
}
