//! The seating below the pool: four numbered positions in clockwise order,
//! the picks filling them from the first, and the ★ on the lead.

use wasm_bindgen_test::*;

use super::{mount_lobby, tals_lobby_with_picks, texts, DAISY, ROLAND, SAM};
use crate::common::YOU;

#[wasm_bindgen_test]
async fn the_seating_numbers_four_positions_and_fills_them_in_order() {
    let picks = [(DAISY, SAM), (ROLAND, YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    assert_eq!(
        texts(&handle, ".seating-position .position-number"),
        ["1", "2", "3", "4"]
    );
    assert_eq!(
        texts(&handle, ".seating-position .position-pick"),
        ["Daisy Walker (Sam)", "Roland Banks (Tal)", "", ""],
        "the picks in seating order, the rest empty"
    );
}

#[wasm_bindgen_test]
async fn the_lead_wears_the_star() {
    // Sam's Daisy was picked first, so she leads.
    let picks = [(DAISY, SAM), (ROLAND, YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    assert_eq!(
        texts(&handle, ".seating-position.lead .position-pick"),
        ["Daisy Walker (Sam)"]
    );
    assert_eq!(texts(&handle, ".seating-position .lead-star"), ["★"]);
}

#[wasm_bindgen_test]
async fn an_empty_seating_has_no_lead() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), YOU, false).await;

    assert_eq!(
        texts(&handle, ".seating-position .position-pick"),
        ["", "", "", ""]
    );
    assert!(texts(&handle, ".seating-position .lead-star").is_empty());
}
