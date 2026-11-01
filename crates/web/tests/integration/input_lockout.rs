//! While the client isn't connected, the board and its in-game modals take no
//! input: they sit in one wrapper carrying the HTML `inert` attribute at every
//! status but `Connected`. The picker and the terminal screens sit outside it,
//! and so does the banner that says why the board is locked.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use leptos::task;
use protocol::SignInMethod;
use wasm_bindgen_test::*;
use web::store::{ClientState, ConnStatus};
use web_sys::Element;

use crate::common::{self, BoardHandle};

/// The lockout wrapper `handle` mounted.
fn wrapper(handle: &BoardHandle) -> Element {
    handle
        .root
        .query_selector(".board-lock")
        .expect("query")
        .expect("the board and its in-game modals sit in a .board-lock wrapper")
}

/// Every status but `Connected`, listed rather than sampled: the claim is
/// "locked at *every* other status".
fn every_status_but_connected() -> [ConnStatus; 8] {
    [
        ConnStatus::Connecting,
        ConnStatus::Reconnecting,
        ConnStatus::Restarting,
        ConnStatus::Failed,
        ConnStatus::AwaitingRoster,
        ConnStatus::VersionMismatch,
        ConnStatus::SignedOut(vec![]),
        ConnStatus::Unresumable,
    ]
}

#[wasm_bindgen_test]
async fn the_board_is_inert_at_every_status_but_connected() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    let lock = wrapper(&handle);
    assert!(
        lock.query_selector(".board").expect("query").is_some(),
        "the board sits inside the wrapper"
    );

    for status in every_status_but_connected() {
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
    assert!(
        el.closest("[inert]").expect("closest").is_none(),
        "{sel} must sit outside the lockout wrapper, so it stays clickable at {status:?}"
    );
    el
}

#[wasm_bindgen_test]
async fn the_picker_and_the_terminal_screens_stay_clickable_while_the_board_is_locked() {
    let dev = SignInMethod {
        label: "Dev sign-in".to_string(),
        url: "/auth/dev".to_string(),
    };
    for (status, sel) in [
        (ConnStatus::AwaitingRoster, ".picker-overlay"),
        (ConnStatus::SignedOut(vec![dev]), ".sign-in"),
        (ConnStatus::VersionMismatch, ".version-mismatch"),
        (ConnStatus::Unresumable, ".unresumable"),
    ] {
        let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
        handle.store.update(|s| s.status = status.clone());
        task::tick().await;

        assert!(
            wrapper(&handle).has_attribute("inert"),
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
        wrapper(&handle).has_attribute("inert"),
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
    // picker and the terminal screens explain themselves, and a first connect
    // has nothing to explain yet.
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;
    for status in every_status_but_connected()
        .into_iter()
        .filter(|s| !matches!(s, ConnStatus::Reconnecting | ConnStatus::Restarting))
        .chain([ConnStatus::Connected])
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
