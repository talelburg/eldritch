//! The prompt's layout on the board: the spinner on the tab of an
//! investigator another account answers for, the waiting strip above the
//! board, and the lead's strip or the lead-waiting line.
//!
//! Each game is [`reassign`]'s: Sam holds Wendy Adams, whose mulligan the game
//! awaits, and Tal, [`YOU`], holds Roland Banks.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{EngineOutcome, InputRequest, Responder};
use game_core::state::investigator::InvestigatorId;
use game_core::test_support;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web_sys::Element;

use crate::common::{BoardHandle, YOU};
use crate::lobby::{self, SAM};
use crate::reassign;

/// Roland Banks, Tal's, in [`reassign::mount_as`]'s game.
const ROLAND: InvestigatorId = InvestigatorId(2);

/// The name on each investigator's tab that carries a spinner.
fn spinning(handle: &BoardHandle) -> Vec<String> {
    let tabs = handle.root.query_selector_all(".inv-tab").expect("query");
    (0..tabs.length())
        .filter_map(|i| tabs.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .filter(|tab| tab.query_selector(".spinner").expect("query").is_some())
        .filter_map(|tab| tab.query_selector(".tab-name").expect("query"))
        .map(|name| name.text_content().unwrap_or_default())
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
    let handle = reassign::mount_as(YOU, None).await;

    assert_eq!(spinning(&handle), vec!["Wendy Adams".to_string()]);
    assert_eq!(
        lobby::texts(&handle, ".waiting-strip"),
        vec![format!(
            "Waiting for Sam: Wendy Adams: {}",
            prompt_text(&handle)
        )]
    );
    assert!(
        lobby::texts(&handle, ".waiting-strip button").is_empty(),
        "the waiting strip offers no controls"
    );
    assert!(
        lobby::texts(&handle, ".prompt-banner").is_empty(),
        "nothing on Tal's prompt bar"
    );
}

#[wasm_bindgen_test]
async fn sam_answering_sees_neither_spinner_nor_waiting_strip() {
    let handle = reassign::mount_as(SAM, None).await;

    assert!(spinning(&handle).is_empty());
    assert!(lobby::texts(&handle, ".waiting-strip").is_empty());
}

/// Swarm of Rats can move two ways, and the lead, Roland, chooses.
fn swarm_moves() -> EngineOutcome {
    test_support::awaiting_request(
        InputRequest::pick_single(
            "The lead investigator chooses where Swarm of Rats moves.",
            vec![],
        )
        .answered_by(Responder::Lead(ROLAND)),
    )
}

#[wasm_bindgen_test]
async fn tal_decides_for_the_table_in_the_lead_strip_above_the_board() {
    let handle = reassign::mount_as(YOU, Some(swarm_moves())).await;

    assert_eq!(
        lobby::texts(&handle, ".board-lock .lead-strip .strip-name"),
        vec!["★ As lead investigator".to_string()]
    );
    assert_eq!(
        lobby::texts(&handle, ".lead-strip .prompt"),
        vec![prompt_text(&handle)]
    );
    assert!(
        lobby::texts(&handle, ".prompt-banner").is_empty(),
        "the lead's decision is in no bottom-bar strip"
    );
    assert!(spinning(&handle).is_empty(), "a lead decision marks no tab");
    assert!(lobby::texts(&handle, ".lead-waiting").is_empty());
}

#[wasm_bindgen_test]
async fn sam_waits_for_the_lead_by_name_and_account() {
    let handle = reassign::mount_as(SAM, Some(swarm_moves())).await;

    assert_eq!(
        lobby::texts(&handle, ".lead-waiting"),
        vec!["Waiting for the lead investigator, Roland Banks (Tal)".to_string()]
    );
    assert!(spinning(&handle).is_empty(), "a lead decision marks no tab");
    assert!(lobby::texts(&handle, ".waiting-strip").is_empty());
}
