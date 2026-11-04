//! A refused lobby command shows as a toast, to the member who sent it.

use game_core::state::CardCode;
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::LobbyRefusal;
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::store;

use super::{mount_lobby, tals_lobby_with_picks, texts, ROLAND, SAM};
use crate::common::{BoardHandle, YOU};

/// Feed `handle`'s store the `LobbyRefused` frame for `reason`.
async fn refused(handle: &BoardHandle, reason: LobbyRefusal) {
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
    let handle = mount_lobby(tals_lobby_with_picks(&[(ROLAND, YOU)]), SAM, false).await;
    assert!(
        texts(&handle, ".toast").is_empty(),
        "no toast before a refusal"
    );

    refused(
        &handle,
        LobbyRefusal::Taken {
            investigator: CardCode::new(ROLAND),
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
async fn a_take_at_a_full_lobby_says_so() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), SAM, false).await;

    refused(&handle, LobbyRefusal::LobbyFull).await;

    assert_eq!(
        texts(&handle, ".toast"),
        ["The lobby is full: 4 investigators are picked."]
    );
}
