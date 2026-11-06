//! The waiting side of the prompt's layout on the board: the spinner on the
//! heading of an investigator another account answers for, the waiting strip
//! above the board, and the line waiting for the lead.
//!
//! Each game is [`reassign`](crate::reassign)'s: Sam holds Wendy Adams, whose
//! mulligan the game awaits, and Tal, [`YOU`], holds Roland Banks.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{EngineOutcome, InputRequest, Responder};
use game_core::state::InvestigatorId;
use game_core::test_support;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::Element;

use crate::common::{BoardHandle, YOU};
use crate::lobby::{texts, SAM};
use crate::reassign::mount_as;

/// Roland Banks, Tal's, in [`mount_as`]'s game.
const ROLAND: InvestigatorId = InvestigatorId(2);

/// The name in each investigator panel's heading whose heading carries a
/// spinner.
fn spinning(handle: &BoardHandle) -> Vec<String> {
    let headings = handle.root.query_selector_all(".inv-name").expect("query");
    (0..headings.length())
        .filter_map(|i| headings.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .filter(|heading| heading.query_selector(".spinner").expect("query").is_some())
        .map(|heading| heading.text_content().unwrap_or_default())
        .collect()
}

/// The live prompt's text.
fn prompt_text(handle: &BoardHandle) -> String {
    handle.store.with_untracked(|s| match &s.outcome {
        Some(EngineOutcome::AwaitingInput { request }) => request.prompt.clone(),
        other => panic!("expected a live prompt, got {other:?}"),
    })
}

#[wasm_bindgen_test]
async fn tal_sees_a_spinner_on_wendy_and_a_waiting_strip_with_no_controls() {
    let handle = mount_as(YOU, None).await;

    assert_eq!(spinning(&handle), vec!["Wendy Adams".to_string()]);
    assert_eq!(
        texts(&handle, ".waiting-strip"),
        vec![format!(
            "Waiting for Sam: Wendy Adams: {}",
            prompt_text(&handle)
        )]
    );
    assert!(
        texts(&handle, ".waiting-strip button").is_empty(),
        "the waiting strip offers no controls"
    );
    assert!(
        texts(&handle, ".prompt-banner").is_empty(),
        "nothing on Tal's prompt bar"
    );
}

#[wasm_bindgen_test]
async fn sam_answering_sees_neither_spinner_nor_waiting_strip() {
    let handle = mount_as(SAM, None).await;

    assert!(spinning(&handle).is_empty());
    assert!(texts(&handle, ".waiting-strip").is_empty());
}

#[wasm_bindgen_test]
async fn sam_waits_for_the_lead_by_name_and_account() {
    let swarm_moves = InputRequest::pick_single(
        "The lead investigator chooses where Swarm of Rats moves.",
        vec![],
    )
    .answered_by(Responder::Lead(ROLAND));
    let handle = mount_as(SAM, Some(test_support::awaiting_request(swarm_moves))).await;

    assert_eq!(
        texts(&handle, ".lead-waiting"),
        vec!["Waiting for the lead investigator, Roland Banks (Tal)".to_string()]
    );
    assert!(
        spinning(&handle).is_empty(),
        "a lead decision marks no panel"
    );
    assert!(texts(&handle, ".waiting-strip").is_empty());
}
