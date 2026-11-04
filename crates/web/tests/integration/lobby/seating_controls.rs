//! The creator's controls on the seating: ◀ ▶ to move a pick along it and ★
//! to make a pick the lead. Every other member sees the seating without them.

use protocol::lobby::Direction;
use protocol::{AccountId, ClientMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::HtmlButtonElement;

use super::{click, mount_lobby, sent, tals_lobby_with_picks, DAISY, ROLAND, SAM};
use crate::common::{BoardHandle, YOU};

/// Sam's Daisy, then Tal's Roland: Daisy leads.
const PICKS: [(&str, AccountId); 2] = [(DAISY, SAM), (ROLAND, YOU)];

/// Whether each `sel` under `handle`'s root is disabled, in document order.
fn disabled(handle: &BoardHandle, sel: &str) -> Vec<bool> {
    let nodes = handle.root.query_selector_all(sel).expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .map(|node| {
            node.dyn_into::<HtmlButtonElement>()
                .expect("a button")
                .disabled()
        })
        .collect()
}

#[wasm_bindgen_test]
async fn the_creator_gets_move_and_lead_controls_on_each_pick() {
    let handle = mount_lobby(tals_lobby_with_picks(&PICKS), YOU, false).await;

    assert_eq!(
        disabled(&handle, ".seating-position button.move-earlier"),
        [true, false],
        "position 1 has nowhere earlier to go"
    );
    assert_eq!(
        disabled(&handle, ".seating-position button.move-later"),
        [false, true],
        "the last pick has nowhere later to go"
    );
    assert_eq!(
        disabled(&handle, ".seating-position button.set-lead").len(),
        1,
        "a ★ to press on Roland alone, since Daisy already leads"
    );
}

#[wasm_bindgen_test]
async fn a_member_who_is_not_the_creator_sees_no_controls() {
    let handle = mount_lobby(tals_lobby_with_picks(&PICKS), SAM, false).await;

    for sel in [
        ".seating-position button.move-earlier",
        ".seating-position button.move-later",
        ".seating-position button.set-lead",
    ] {
        assert!(disabled(&handle, sel).is_empty(), "Sam sees no {sel}");
    }
}

#[wasm_bindgen_test]
async fn moving_roland_earlier_sends_the_move() {
    let mut handle = mount_lobby(tals_lobby_with_picks(&PICKS), YOU, false).await;

    click(
        &handle,
        ".seating-position:nth-child(2) button.move-earlier",
    );

    match sent(&mut handle) {
        ClientMessage::MoveInSeating {
            investigator,
            direction,
        } => {
            assert_eq!(investigator.as_str(), ROLAND);
            assert_eq!(direction, Direction::Earlier);
        }
        other => panic!("expected a MoveInSeating, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn starring_roland_sends_the_lead() {
    let mut handle = mount_lobby(tals_lobby_with_picks(&PICKS), YOU, false).await;

    click(&handle, ".seating-position button.set-lead");

    match sent(&mut handle) {
        ClientMessage::SetLead { investigator } => assert_eq!(investigator.as_str(), ROLAND),
        other => panic!("expected a SetLead, got {other:?}"),
    }
}
