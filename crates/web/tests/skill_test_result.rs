//! Headless tests for `SkillTestResultView` (#478, made a modal by #541): feed a
//! `SkillTestStarted` batch (captures difficulty) then a resolution batch (chaos
//! token + outcome) through the store, and assert the modal renders the token,
//! total-vs-difficulty and outcome lines, carries the acknowledge pause's **only**
//! Confirm, and cannot be dismissed by anything else. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenResolution};
use game_core::test_support::fixtures::test_investigator;
use game_core::{EngineOutcome, Event, InputResponse, PlayerAction};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::skill_test_result::SkillTestResultView;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

fn base_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build()
}

/// The cosmetic acknowledge pause: an option-less, **un-anchored** `Confirm`
/// (ADR 0011). The modal only renders while this is live, which is what
/// guarantees its Confirm always has real engine input to submit.
fn acknowledge_pause() -> EngineOutcome {
    game_core::test_support::fixtures::awaiting_confirm_input("Acknowledge the skill-test result.")
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
    let (store, _rx) = mount_modal();

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
                outcome: acknowledge_pause(),
            },
        );
    });
    leptos::task::tick().await;

    let section = last_section().expect("the result modal renders after resolution");
    let text = section.text_content().unwrap_or_default();
    assert!(text.contains("Chaos token"), "shows the token line: {text}");
    assert!(text.contains("Total 5"), "shows total: {text}");
    assert!(text.contains("difficulty 3"), "shows difficulty: {text}");
    assert!(text.contains("Succeeded by 2"), "shows outcome: {text}");
}

/// #787: a symbol token whose ST.4 effect suspends for input (The Gathering's
/// Tablet dealing damage with a soak target in play) puts the reveal in an
/// earlier batch than the outcome. The modal must still name it, not show the
/// em-dash fallback.
#[wasm_bindgen_test]
async fn names_a_token_revealed_in_an_earlier_batch() {
    let (store, _rx) = mount_modal();

    store.update(|s| {
        // Batch 1: the test starts at difficulty 3.
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
        // Batch 2: the Tablet is revealed and its ST.4 damage suspends for the
        // soak prompt — no outcome yet.
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![Event::ChaosTokenRevealed {
                    token: ChaosToken::Tablet,
                    resolution: TokenResolution::Modifier(-2),
                }],
                outcome: game_core::test_support::fixtures::awaiting_confirm_input(
                    "Assign the damage.",
                ),
            },
        );
        // Batch 3: the resumed test resolves. No reveal in this batch.
        reduce(
            s,
            ServerMessage::Applied {
                state: Box::new(base_game()),
                events: vec![Event::SkillTestFailed {
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    reason: game_core::FailureReason::Total,
                    by: 2,
                }],
                outcome: acknowledge_pause(),
            },
        );
    });
    leptos::task::tick().await;

    let section = last_section().expect("the result modal renders after resolution");
    let token_line = section
        .query_selector(".str-token")
        .expect("query")
        .expect("the modal has a token line")
        .text_content()
        .unwrap_or_default();
    assert!(
        token_line.contains("Tablet (-2)"),
        "names the token revealed two batches earlier: {token_line}"
    );
    assert!(
        !token_line.contains('—'),
        "no em-dash fallback: {token_line}"
    );
    let text = section.text_content().unwrap_or_default();
    assert!(text.contains("Failed by 2"), "shows outcome: {text}");
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
    let (_store, _rx) = mount_modal();
    leptos::task::tick().await;
    assert_eq!(
        count(),
        before,
        "an empty store renders no result modal section"
    );
}

/// Mount the modal with a fresh store and a capturing outbound channel.
fn mount_modal() -> (
    RwSignal<ClientState>,
    mpsc::UnboundedReceiver<ClientMessage>,
) {
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <SkillTestResultView/> }
    });
    (store, rx)
}

/// Drive `store` to a resolved test (difficulty 3, +1 token, succeeded by 2)
/// with `outcome` live in the final batch.
async fn resolve_a_test(store: RwSignal<ClientState>, outcome: EngineOutcome) {
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
                outcome,
            },
        );
    });
    leptos::task::tick().await;
}

#[wasm_bindgen_test]
async fn its_confirm_submits_the_acknowledge() {
    let (store, mut rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    last_section()
        .expect("the modal renders")
        .query_selector(".str-confirm")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("the modal carries its own Confirm")
        .click();
    leptos::task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Confirm),
        other @ ClientMessage::Submit { .. } => panic!("expected Confirm, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn the_backdrop_does_not_dismiss_it() {
    // Dismissal submits real engine input, so a stray click on the scrim must not
    // advance the game on the player's behalf: the scrim carries no handler, and
    // clicking it leaves the modal exactly where it was.
    let (store, mut rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    document()
        .query_selector_all(".str-backdrop")
        .expect("query")
        .item(0)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a scrim renders behind the modal")
        .click();
    leptos::task::tick().await;
    assert!(rx.try_recv().is_err(), "a backdrop click submits nothing");
    assert!(
        last_section().is_some(),
        "the modal is still up after a backdrop click"
    );
}

#[wasm_bindgen_test]
async fn no_modal_without_a_live_acknowledge() {
    // A resolved test whose pause is off (or already answered) must not leave a
    // modal with no way out.
    let before = document()
        .query_selector_all(".skill-test-result")
        .expect("query")
        .length();
    let (store, _rx) = mount_modal();
    resolve_a_test(store, EngineOutcome::Done).await;
    assert_eq!(
        document()
            .query_selector_all(".skill-test-result")
            .expect("query")
            .length(),
        before,
        "no live Confirm ⇒ no modal (it would be un-dismissible)"
    );
}
