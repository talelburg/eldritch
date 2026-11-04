//! While the client isn't connected, the board and its in-game modals take no
//! input: they sit in one wrapper carrying the HTML `inert` attribute at every
//! status but `Connected`. The home page, the lobby and the terminal screens
//! sit outside it, and so does the banner that says why the board is locked.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::scenario::ScenarioId;
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::{Lobby, Member};
use protocol::SignInMethod;
use wasm_bindgen_test::*;
use web::store::{ClientState, ConnStatus};
use web_sys::Element;

use crate::common::{self, BoardHandle};

#[wasm_bindgen_test]
async fn the_board_is_inert_at_every_status_but_connected() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    let lock = handle.lock();
    assert!(
        lock.query_selector(".board").expect("query").is_some(),
        "the board sits inside the wrapper"
    );

    for status in common::every_status()
        .into_iter()
        .filter(|s| *s != ConnStatus::Connected)
    {
        handle.store.update(|s| s.status = status.clone());
        task::tick().await;
        assert!(
            lock.has_attribute("inert"),
            "{status:?} must leave the board inert"
        );
    }

    handle.store.update(|s| s.status = ConnStatus::Connected);
    task::tick().await;
    assert!(
        !lock.has_attribute("inert"),
        "Connected must leave the board live"
    );
}

/// Assert `sel` renders under `handle` with no `inert` ancestor, so it takes
/// clicks while the board is locked; return it.
fn rendered_and_live(handle: &BoardHandle, sel: &str, status: &ConnStatus) -> Element {
    let el = handle
        .root
        .query_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("{status:?} must render {sel}"));
    common::assert_live(&el, &format!("{sel} at {status:?}"));
    el
}

#[wasm_bindgen_test]
async fn the_pre_game_and_terminal_screens_stay_clickable_while_the_board_is_locked() {
    let dev = SignInMethod {
        label: "Dev sign-in".to_string(),
        url: "/auth/dev".to_string(),
    };
    // A lobby renders only with one in the store, which no other screen reads.
    let lobby = Lobby::new(
        ScenarioId::new("the-gathering"),
        common::YOU,
        vec![Member {
            account: common::YOU,
            display_name: "Tal".to_string(),
        }],
    )
    .view();
    for (status, sel) in [
        (ConnStatus::Home, ".home"),
        (ConnStatus::InLobby, ".lobby"),
        (ConnStatus::SignedOut(vec![dev]), ".sign-in"),
        (ConnStatus::VersionMismatch, ".version-mismatch"),
        (ConnStatus::Unresumable, ".unresumable"),
    ] {
        let state = ClientState {
            lobby: Some(lobby.clone()),
            ..ClientState::default()
        };
        let handle = common::mount_board(state, &cards::TEST_REGISTRY).await;
        handle.store.update(|s| s.status = status.clone());
        task::tick().await;

        assert!(
            handle.lock().has_attribute("inert"),
            "{status:?} locks the board behind {sel}"
        );
        rendered_and_live(&handle, sel, &status);
    }
}

#[wasm_bindgen_test]
async fn a_disconnect_shows_reconnecting_over_the_locked_board() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    handle.store.update(|s| s.status = ConnStatus::Reconnecting);
    task::tick().await;

    let banner = rendered_and_live(&handle, ".lock-banner", &ConnStatus::Reconnecting);
    assert_eq!(
        banner.text_content().as_deref(),
        Some("Reconnecting…"),
        "the banner says why the board is locked"
    );
}

#[wasm_bindgen_test]
async fn a_server_restart_shows_server_restarting_over_the_locked_board() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    handle.store.update(|s| s.status = ConnStatus::Restarting);
    task::tick().await;

    assert!(
        handle.lock().has_attribute("inert"),
        "a restart locks the board"
    );
    let banner = rendered_and_live(&handle, ".lock-banner", &ConnStatus::Restarting);
    assert_eq!(
        banner.text_content().as_deref(),
        Some("Server restarting…"),
        "the banner says the server is restarting"
    );
}

#[wasm_bindgen_test]
async fn no_lock_banner_shows_at_any_other_status() {
    // `Reconnecting` and `Restarting` are the statuses with a banner: the
    // pre-game and terminal screens explain themselves, and a first connect
    // has nothing to explain yet.
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    for status in common::every_status()
        .into_iter()
        .filter(|s| !s.is_retrying())
    {
        handle.store.update(|s| s.status = status.clone());
        task::tick().await;
        assert!(
            handle
                .root
                .query_selector(".lock-banner")
                .expect("query")
                .is_none(),
            "{status:?} must show no lock banner"
        );
    }
}
