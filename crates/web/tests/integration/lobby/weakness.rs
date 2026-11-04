//! A pick's weakness choice: a checkbox on your own pick, and the choice shown
//! read-only in another member's deck.

use cards::decks::WeaknessChoice;
use leptos::task;
use protocol::lobby::LobbyView;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::HtmlInputElement;

use super::{
    click, entry, mount_lobby, one, sent, tals_lobby_with_picks, texts, DAISY, ROLAND, SAM,
};
use crate::common::{BoardHandle, YOU};

/// `view` with `investigator`'s pick set to `choice`.
fn with_weakness(mut view: LobbyView, investigator: &str, choice: WeaknessChoice) -> LobbyView {
    view.picks
        .iter_mut()
        .find(|pick| pick.investigator.as_str() == investigator)
        .expect("the investigator is picked")
        .weakness = choice;
    view
}

/// Whether the weakness checkbox on `investigator`'s entry is checked.
fn checked(handle: &BoardHandle, investigator: &str) -> bool {
    one(handle, &entry(investigator, "input.random-basic-weakness"))
        .dyn_into::<HtmlInputElement>()
        .expect("an input")
        .checked()
}

#[wasm_bindgen_test]
async fn checking_random_basic_weakness_on_your_pick_sends_random() {
    let mut handle = mount_lobby(tals_lobby_with_picks(&[(ROLAND, YOU)]), YOU, false).await;
    assert!(!checked(&handle, ROLAND), "a new pick's weakness is fixed");

    click(&handle, &entry(ROLAND, "input.random-basic-weakness"));
    task::tick().await;

    match sent(&mut handle) {
        ClientMessage::SetWeakness {
            investigator,
            choice,
        } => {
            assert_eq!(investigator.as_str(), ROLAND);
            assert_eq!(choice, WeaknessChoice::Random);
        }
        other => panic!("expected a SetWeakness, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn unchecking_random_basic_weakness_on_your_pick_sends_fixed() {
    let view = with_weakness(
        tals_lobby_with_picks(&[(ROLAND, YOU)]),
        ROLAND,
        WeaknessChoice::Random,
    );
    let mut handle = mount_lobby(view, YOU, false).await;
    assert!(checked(&handle, ROLAND));

    click(&handle, &entry(ROLAND, "input.random-basic-weakness"));
    task::tick().await;

    match sent(&mut handle) {
        ClientMessage::SetWeakness { choice, .. } => assert_eq!(choice, WeaknessChoice::Fixed),
        other => panic!("expected a SetWeakness, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn another_members_fixed_weakness_is_listed_read_only() {
    let handle = mount_lobby(tals_lobby_with_picks(&[(DAISY, SAM)]), YOU, false).await;

    // Daisy's default deck's fixed weakness: Paranoia 01097.
    assert_eq!(
        texts(&handle, &entry(DAISY, ".pool-deck li.pool-weakness")),
        ["Paranoia"]
    );
    assert!(
        texts(&handle, &entry(DAISY, "input")).is_empty(),
        "another member's choice offers no control"
    );
}

#[wasm_bindgen_test]
async fn another_members_random_weakness_is_listed_in_place_of_the_fixed_one() {
    let view = with_weakness(
        tals_lobby_with_picks(&[(DAISY, SAM)]),
        DAISY,
        WeaknessChoice::Random,
    );
    let handle = mount_lobby(view, YOU, false).await;

    assert_eq!(
        texts(&handle, &entry(DAISY, ".pool-deck li.pool-weakness")),
        ["Random basic weakness"]
    );
    let deck = texts(&handle, &entry(DAISY, ".pool-deck li"));
    assert!(
        !deck.iter().any(|card| card.contains("Paranoia")),
        "{deck:?}"
    );
    assert!(texts(&handle, &entry(DAISY, "input")).is_empty());
}
