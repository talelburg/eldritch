//! The seating below the pool: four numbered positions in clockwise order,
//! the picks filling them from the first, and the ★ on the lead.

use wasm_bindgen_test::*;

use super::{mount_lobby, tals_lobby_with_picks, texts, SAM};
use crate::common::YOU;

#[wasm_bindgen_test]
async fn the_seating_numbers_four_positions_and_fills_them_in_order() {
    let picks = [("01002", SAM), ("01001", YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    assert_eq!(texts(&handle, ".seat .seat-number"), ["1", "2", "3", "4"]);
    assert_eq!(
        texts(&handle, ".seat .seat-pick"),
        ["Daisy Walker (Sam)", "Roland Banks (Tal)", "", ""],
        "the picks in seating order, the rest empty"
    );
}

#[wasm_bindgen_test]
async fn the_lead_wears_the_star() {
    // Sam's Daisy was picked first, so she leads.
    let picks = [("01002", SAM), ("01001", YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    assert_eq!(
        texts(&handle, ".seat.lead .seat-pick"),
        ["Daisy Walker (Sam)"]
    );
    assert_eq!(texts(&handle, ".seat .lead-star"), ["★"]);
}

#[wasm_bindgen_test]
async fn an_empty_seating_has_no_lead() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), YOU, false).await;

    assert_eq!(texts(&handle, ".seat .seat-pick"), ["", "", "", ""]);
    assert!(texts(&handle, ".seat .lead-star").is_empty());
}
