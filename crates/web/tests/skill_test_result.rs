//! Headless test for `SkillTestResultView` (#478): feed a `SkillTestStarted`
//! batch (captures difficulty) then a resolution batch (chaos token + outcome)
//! through the store, and assert the panel renders the token, total-vs-difficulty,
//! and outcome lines. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::state::{ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenResolution};
use game_core::test_support::fixtures::test_investigator;
use game_core::{EngineOutcome, Event};
use leptos::prelude::*;
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::skill_test_result::SkillTestResultView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

fn base_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build()
}

fn last_section() -> Option<web_sys::Element> {
    let secs = leptos::prelude::document()
        .query_selector_all(".skill-test-result")
        .expect("query");
    let n = secs.length();
    if n == 0 {
        return None;
    }
    Some(
        secs.item(n - 1)
            .expect("present")
            .dyn_into::<web_sys::Element>()
            .expect("Element"),
    )
}

#[wasm_bindgen_test]
async fn renders_token_total_and_outcome_after_resolution() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <SkillTestResultView/> }
    });

    // Batch 1: the test started at difficulty 3 (captures difficulty).
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![Event::SkillTestStarted {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    difficulty: 3,
                }],
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    // Batch 2: resolution — +1 token, succeeded by 2 (total 5).
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![
                    Event::ChaosTokenRevealed {
                        token: ChaosToken::Numeric(1),
                        resolution: TokenResolution::Modifier(1),
                    },
                    Event::SkillTestSucceeded {
                        investigator: InvestigatorId(1),
                        skill: SkillKind::Willpower,
                        margin: 2,
                    },
                ],
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    let section = last_section().expect("the result panel renders after resolution");
    let text = section.text_content().unwrap_or_default();
    assert!(text.contains("Chaos token"), "shows the token line: {text}");
    assert!(text.contains("Total 5"), "shows total: {text}");
    assert!(text.contains("difficulty 3"), "shows difficulty: {text}");
    assert!(text.contains("Succeeded by 2"), "shows outcome: {text}");
}

#[wasm_bindgen_test]
async fn renders_nothing_before_any_resolution() {
    // Other tests on the same page accumulate panels in the DOM, so assert on the
    // before/after delta for THIS mount rather than an absolute count.
    let count = || {
        leptos::prelude::document()
            .query_selector_all(".skill-test-result")
            .expect("query")
            .length()
    };
    let before = count();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <SkillTestResultView/> }
    });
    leptos::task::tick().await;
    assert_eq!(
        count(),
        before,
        "an empty store renders no result panel section"
    );
}
