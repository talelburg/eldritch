//! Delete in the lobby header's ⋯ and the board's: the creator's alone, and
//! it asks first. Confirming deletes the game through the [`DeleteGame`]
//! context, which these tests' mount records; cancelling deletes nothing.
//!
//! [`DeleteGame`]: web::delete_game::DeleteGame
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use leptos::task;
use protocol::GameId;
use wasm_bindgen_test::*;

use crate::common::{BoardHandle, YOU};
use crate::lobby::more_menu::{choose, open_menu};
use crate::lobby::{click, mount_lobby, tals_lobby_with_sam, texts, GAME_ID, SAM};
use crate::reassign;

/// What the confirmation asks.
const ASKS: &str = "Delete for everyone? This can't be undone.";

/// The question the open confirmation asks, if one is open.
fn asked(handle: &BoardHandle) -> Vec<String> {
    texts(handle, ".confirm-dialog .confirm-question")
}

/// The game the mount's delete recorder was handed, if any, and that no
/// other was.
fn deleted(handle: &mut BoardHandle) -> Option<GameId> {
    let game_id = handle.deleted.try_recv().ok();
    assert!(handle.deleted.try_recv().is_err(), "at most one delete");
    game_id
}

#[wasm_bindgen_test]
async fn the_lobbys_delete_asks_first_and_deletes_only_once_confirmed() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    open_menu(&handle).await;

    choose(&handle, "Delete").await;

    assert_eq!(asked(&handle), [ASKS]);
    assert_eq!(deleted(&mut handle), None, "nothing is deleted yet");

    click(&handle, ".confirm-dialog .confirm-delete");
    task::tick().await;

    assert_eq!(deleted(&mut handle), Some(GameId::new(GAME_ID)));
    assert!(asked(&handle).is_empty(), "the confirmation closes");
}

#[wasm_bindgen_test]
async fn cancelling_the_confirmation_deletes_nothing() {
    let mut handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;
    open_menu(&handle).await;
    choose(&handle, "Delete").await;

    click(&handle, ".confirm-dialog .confirm-cancel");
    task::tick().await;

    assert!(asked(&handle).is_empty(), "the confirmation closes");
    assert_eq!(deleted(&mut handle), None);
}

#[wasm_bindgen_test]
async fn a_lobby_members_menu_has_no_delete() {
    let handle = mount_lobby(tals_lobby_with_sam(), SAM, false).await;

    assert!(!open_menu(&handle).await.contains(&"Delete".to_string()));
}

/// The plain entries of the board's open ⋯ menu, outside any submenu.
fn board_entries(handle: &BoardHandle) -> Vec<String> {
    texts(handle, ".board-menu .more-menu-items > .menu-item")
}

/// A finished game offers no reassignment, so Delete is all its menu has.
#[wasm_bindgen_test]
async fn the_boards_delete_is_the_creators_even_on_a_finished_game() {
    let mut handle = reassign::mount_as(YOU, Some(EngineOutcome::Done)).await;
    reassign::open_menu(&handle).await;
    assert_eq!(board_entries(&handle), ["Delete"]);

    click(&handle, ".board-menu .more-menu-items > .menu-item");
    task::tick().await;
    assert_eq!(asked(&handle), [ASKS]);
    click(&handle, ".confirm-dialog .confirm-delete");
    task::tick().await;

    assert_eq!(
        deleted(&mut handle),
        Some(GameId::new(reassign::GAME_ID))
    );
}

#[wasm_bindgen_test]
async fn a_holders_board_menu_has_no_delete() {
    let handle = reassign::mount_as(SAM, None).await;
    reassign::open_menu(&handle).await;

    assert!(board_entries(&handle).is_empty());
}
