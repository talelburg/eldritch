//! A refused lobby command shows as a toast, to the member who sent it.

use game_core::state::CardCode;
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::LobbyRefusal;
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::store;

use super::{mount_lobby, tals_lobby_with_picks, texts, SAM};
use crate::common::YOU;

/// Feed `handle`'s store the `LobbyRefused` frame for `reason`.
async fn refused(handle: &crate::common::BoardHandle, reason: LobbyRefusal) {
    let frame = serde_json::to_string(&ServerMessage::LobbyRefused { reason })
        .expect("a LobbyRefused serializes");
    handle.store.update(|s| {
        store::receive(s, &frame);
    });
    task::tick().await;
}

#[wasm_bindgen_test]
async fn a_take_beaten_to_it_names_who_took_the_investigator() {
    // Tal took Roland a moment before Sam's Take arrived.
    let handle = mount_lobby(tals_lobby_with_picks(&[("01001", YOU)]), SAM, false).await;
    assert!(
        texts(&handle, ".toast").is_empty(),
        "no toast before a refusal"
    );

    refused(
        &handle,
        LobbyRefusal::Taken {
            investigator: CardCode::new("01001"),
            by: YOU,
        },
    )
    .await;

    assert_eq!(
        texts(&handle, ".toast"),
        ["Roland Banks was just taken by Tal."]
    );
}

#[wasm_bindgen_test]
async fn a_take_at_a_full_table_says_so() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), SAM, false).await;

    refused(&handle, LobbyRefusal::TableFull).await;

    assert_eq!(
        texts(&handle, ".toast"),
        ["The table is full: four investigators are picked."]
    );
}
