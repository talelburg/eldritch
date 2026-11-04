//! Each investigator of the pool is free (Take), yours (Give up), or held by
//! another member (their name and its default deck, read-only).

use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::HtmlButtonElement;

use super::{
    click, entry, mount_lobby, one, sent, tals_lobby_with_picks, texts, AGNES, DAISY, ROLAND, SAM,
    SKIDS, WENDY,
};
use crate::common::YOU;

#[wasm_bindgen_test]
async fn a_free_investigator_is_taken_with_take() {
    let mut handle = mount_lobby(tals_lobby_with_picks(&[]), YOU, false).await;
    assert_eq!(texts(&handle, &entry(ROLAND, ".pool-state")), ["Free"]);

    click(&handle, &entry(ROLAND, "button.take"));
    task::tick().await;

    match sent(&mut handle) {
        ClientMessage::Take { investigator } => assert_eq!(investigator.as_str(), ROLAND),
        other => panic!("expected a Take, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn your_pick_is_given_up_with_give_up() {
    let mut handle = mount_lobby(tals_lobby_with_picks(&[(ROLAND, YOU)]), YOU, false).await;
    assert_eq!(texts(&handle, &entry(ROLAND, ".pool-state")), ["Yours"]);
    assert!(texts(&handle, &entry(ROLAND, "button.take")).is_empty());

    click(&handle, &entry(ROLAND, "button.give-up"));
    task::tick().await;

    match sent(&mut handle) {
        ClientMessage::GiveUp { investigator } => assert_eq!(investigator.as_str(), ROLAND),
        other => panic!("expected a GiveUp, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn another_members_pick_shows_who_holds_it_and_its_deck_read_only() {
    let handle = mount_lobby(tals_lobby_with_picks(&[(DAISY, SAM)]), YOU, false).await;

    assert_eq!(texts(&handle, &entry(DAISY, ".pool-state")), ["Sam"]);
    let deck = texts(&handle, &entry(DAISY, ".pool-deck li"));
    // Daisy's signatures, from her default deck: 01008 and 01009.
    assert!(
        deck.iter().any(|card| card.contains("Daisy's Tote Bag")),
        "{deck:?}"
    );
    assert!(
        deck.iter().any(|card| card.contains("The Necronomicon")),
        "{deck:?}"
    );
    assert!(
        texts(&handle, &entry(DAISY, "button")).is_empty(),
        "another member's pick offers no control"
    );
}

#[wasm_bindgen_test]
async fn take_is_disabled_once_four_investigators_are_picked() {
    let picks = [(ROLAND, YOU), (DAISY, SAM), (SKIDS, SAM), (AGNES, YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    let take = one(&handle, &entry(WENDY, "button.take"))
        .dyn_into::<HtmlButtonElement>()
        .expect("a button");
    assert!(take.disabled(), "the lobby is full");
}

#[wasm_bindgen_test]
async fn take_is_enabled_while_the_lobby_has_room() {
    let picks = [(ROLAND, YOU), (DAISY, SAM), (SKIDS, SAM)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    let take = one(&handle, &entry(WENDY, "button.take"))
        .dyn_into::<HtmlButtonElement>()
        .expect("a button");
    assert!(!take.disabled());
}
