//! An unresumable game explains itself over the board, rather than vanishing:
//! the terminal `Unresumable` status shows its own overlay, with a way to move
//! on to a new game, outside the lockout wrapper.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use leptos::task;
use protocol::{GameId, ReplayCause, UnresumableCause};
use wasm_bindgen_test::*;
use web::store::{ClientState, ConnStatus};

use crate::common::{self, BoardHandle};
use crate::your_games;

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
    assert!(
        card.query_selector(".ur-kept").expect("query").is_none(),
        "with no listing entry for the game, the card adds nothing: {text}"
    );
}

/// The text of the `.unresumable` card under `handle`.
fn card_text(handle: &BoardHandle) -> String {
    handle
        .root
        .query_selector(".unresumable")
        .expect("query")
        .expect("an .unresumable card")
        .text_content()
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn a_members_unresumable_game_adds_where_it_stopped_both_builds_and_that_it_is_kept() {
    let stale = your_games::stale(
        "g-stale",
        1_000,
        UnresumableCause::Replay {
            seq: 214,
            cause: ReplayCause::Rejected,
        },
    );
    let other = your_games::stale("g-other", 2_000, UnresumableCause::Fingerprint);
    let client = ClientState {
        status: ConnStatus::Unresumable,
        game_id: Some(GameId::new("g-stale")),
        games: Some(vec![other, stale]),
        ..ClientState::default()
    };
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;

    let text = card_text(&handle);
    for shown in [
        "This game can't be resumed on this server version",
        "Stopped at action 215 of 340: the engine now refuses that answer",
        "older-build",
        "newer-build",
        "The game is kept as it was: if the server rolls back to its build, it opens again.",
    ] {
        assert!(text.contains(shown), "the card shows {shown}: {text}");
    }
    assert!(
        !text.contains("Setup now deals differently"),
        "only this game's reason: {text}"
    );
    assert!(
        handle
            .root
            .query_selector(".unresumable .ur-new-game")
            .expect("query")
            .is_some(),
        "the New game button stays"
    );
}

#[wasm_bindgen_test]
async fn a_game_missing_from_the_listing_shows_the_sentence_alone() {
    let client = ClientState {
        status: ConnStatus::Unresumable,
        game_id: Some(GameId::new("g-not-mine")),
        games: Some(vec![your_games::stale(
            "g-stale",
            1_000,
            UnresumableCause::Fingerprint,
        )]),
        ..ClientState::default()
    };
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;

    assert_eq!(
        card_text(&handle).trim(),
        "This game can't be resumed on this server versionNew game"
    );
}
