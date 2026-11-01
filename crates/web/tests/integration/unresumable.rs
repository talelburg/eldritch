//! An unresumable game explains itself over the board, rather than vanishing:
//! the terminal `Unresumable` status shows its own overlay, with a way to move
//! on to a new game, outside the lockout wrapper.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use leptos::task;
use wasm_bindgen_test::*;
use web::store::{ClientState, ConnStatus};

use crate::common;

#[wasm_bindgen_test]
async fn the_overlay_is_absent_at_every_status_but_unresumable() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    for status in common::every_status()
        .into_iter()
        .filter(|s| *s != ConnStatus::Unresumable)
    {
        handle.store.update(|s| s.status = status.clone());
        task::tick().await;

        for sel in [".unresumable", ".ur-backdrop"] {
            assert!(
                handle.root.query_selector(sel).expect("query").is_none(),
                "{status:?} must render no {sel}"
            );
        }
    }
}

#[wasm_bindgen_test]
async fn an_unresumable_game_says_why_and_offers_a_new_game_outside_the_lockout() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    handle.store.update(|s| s.status = ConnStatus::Unresumable);
    task::tick().await;

    let card = handle
        .root
        .query_selector(".unresumable")
        .expect("query")
        .expect("an .unresumable card");
    let text = card.text_content().unwrap_or_default();
    assert!(
        text.contains("This game can't be resumed on this server version"),
        "the card says why the game won't load: {text}"
    );
    let button = card
        .query_selector(".ur-new-game")
        .expect("query")
        .expect("the card carries a New game button");
    assert_eq!(button.text_content().as_deref(), Some("New game"));
    assert!(
        handle
            .root
            .query_selector(".ur-backdrop")
            .expect("query")
            .is_some(),
        "the card sits on a scrim over the board"
    );

    assert!(
        handle.lock().has_attribute("inert"),
        "the board behind it is locked"
    );
    common::assert_live(&button, "the New game button");
}
