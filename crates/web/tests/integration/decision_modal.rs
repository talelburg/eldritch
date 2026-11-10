//! Headless tests for `DecisionView`, the decision modal, mounted on its own.
#![cfg(target_arch = "wasm32")]

use game_core::engine::{ChoiceOption, InputRequest, OptionId, Responder};
use game_core::state::InvestigatorId;
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use wasm_bindgen_test::*;
use web::decision::DecisionView;
use web::registry;

use crate::common;

/// A branch's label draws its card-text symbol token as the game's glyph, and
/// not as the bracketed token.
#[wasm_bindgen_test]
async fn a_symbol_token_in_a_branch_label_renders_as_its_glyph() {
    let (store, container) = common::mount(|| {
        registry::provide_cards(&test_support::TEST_REGISTRY);
        leptos::view! { <DecisionView/> }
    });
    store.update(|s| {
        s.outcome = Some(test_support::awaiting_request(
            InputRequest::pick_single(
                "Choose one",
                vec![ChoiceOption::new(OptionId(0), common::INTELLECT_LABEL)],
            )
            .deciding()
            .answered_by(Responder::Investigator(InvestigatorId(1))),
        ));
        common::you_hold_every_investigator(s);
    });
    task::tick().await;
    let branch = common::one(&container, ".decision-modal .decision-branch");
    common::assert_shows_intellect_label(&branch);
}
