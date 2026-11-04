//! The pool: the five default-deck investigators, each marked with who holds
//! them.

use cards::decks;
use wasm_bindgen_test::*;

use super::{mount_lobby, tals_lobby_with_sam, texts};
use crate::common::YOU;

#[wasm_bindgen_test]
async fn the_pool_lists_the_five_investigators_all_free() {
    let handle = mount_lobby(tals_lobby_with_sam(), YOU, false).await;

    let names = texts(&handle, ".pool-entry .pool-name");
    let expected: Vec<String> = decks::DEFAULT_DECKS
        .iter()
        .map(|deck| {
            cards::by_code(deck.investigator)
                .expect("a default deck's investigator is in the corpus")
                .name
                .clone()
        })
        .collect();
    assert_eq!(
        names, expected,
        "the default decks' investigators, in order"
    );
    assert_eq!(texts(&handle, ".pool-entry .pool-state"), ["Free"; 5]);
}
