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

/// Dispatch one pointer event at `(x, y)` on `el`, as a real gesture would.
///
/// The gesture itself is synthesised, but pointer *capture* is not exercised —
/// the events are dispatched straight at the modal, and the capture call the
/// handler makes on a synthetic pointer id is allowed to fail. What is under
/// test is this module's response to the gesture, not the browser's plumbing.
fn pointer_at(el: &web_sys::Element, kind: &str, x: i32, y: i32) {
    let init = web_sys::PointerEventInit::new();
    init.set_bubbles(true);
    init.set_client_x(x);
    init.set_client_y(y);
    let ev = web_sys::PointerEvent::new_with_event_init_dict(kind, &init)
        .expect("construct a pointer event");
    el.dispatch_event(&ev).expect("dispatch");
}

/// Drag `el` from `(from_x, from_y)` to `(to_x, to_y)`.
fn drag(el: &web_sys::Element, from: (i32, i32), to: (i32, i32)) {
    pointer_at(el, "pointerdown", from.0, from.1);
    pointer_at(el, "pointermove", to.0, to.1);
    pointer_at(el, "pointerup", to.0, to.1);
}

fn style_of(el: &web_sys::Element) -> String {
    el.get_attribute("style").unwrap_or_default()
}

/// #857: the modal covers the board, and dismissing it submits engine input, so
/// the only way to check the board first is to shove it aside. Dragging must not
/// break the way out — the Confirm still submits after the modal has been moved.
#[wasm_bindgen_test]
async fn its_confirm_still_submits_after_the_modal_has_been_dragged() {
    let (store, mut rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    let section = last_section().expect("the modal renders");

    drag(&section, (200, 200), (320, 140));
    leptos::task::tick().await;
    assert!(
        style_of(&section).contains("calc(-50% + 120px)"),
        "the modal stays where it was put: {}",
        style_of(&section)
    );

    section
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

/// Dragging is a request to see the board underneath, so the scrim fades with
/// the modal's move rather than needing a second gesture — but it keeps a tint,
/// because the board is still inert.
#[wasm_bindgen_test]
async fn the_scrim_fades_once_the_modal_has_been_moved() {
    let (store, _rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    let section = last_section().expect("the modal renders");
    let scrims = document()
        .query_selector_all(".str-backdrop")
        .expect("query");
    let scrim = scrims
        .item(scrims.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a scrim renders behind the modal");

    assert!(
        style_of(&scrim).contains("0.45"),
        "full strength while centred: {}",
        style_of(&scrim)
    );

    drag(&section, (200, 200), (280, 200));
    leptos::task::tick().await;
    let faded = style_of(&scrim);
    assert!(
        !faded.contains("0.45") && faded.contains("rgba(0, 0, 0, 0."),
        "fades to a lighter tint, but keeps one: {faded}"
    );
}

/// A press that lands on the Confirm button starts no gesture: the button is the
/// modal's only way out, and the drag surface must not swallow it.
#[wasm_bindgen_test]
async fn a_press_on_confirm_does_not_drag_the_modal() {
    let (store, _rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    let section = last_section().expect("the modal renders");
    let confirm = section
        .query_selector(".str-confirm")
        .expect("query")
        .expect("the modal carries its own Confirm");

    drag(&confirm, (200, 200), (320, 140));
    leptos::task::tick().await;
    let style = style_of(&section);
    assert!(
        style.contains("calc(-50% + 0px)"),
        "the modal has not moved: {style}"
    );
}

/// Drag state belongs to the prompt, not to the mount: a modal parked aside must
/// not strand the next prompt where the player cannot see it.
///
/// The second test resolves in the same update as the first is acknowledged, so
/// the modal never goes away in between — the case a reset keyed on liveness
/// alone would miss.
#[wasm_bindgen_test]
async fn a_newly_opened_modal_is_centred_again() {
    let (store, _rx) = mount_modal();
    resolve_a_test(store, acknowledge_pause()).await;
    let section = last_section().expect("the modal renders");
    drag(&section, (200, 200), (320, 140));
    leptos::task::tick().await;
    assert!(style_of(&section).contains("120px"), "moved first");

    resolve_a_test(store, acknowledge_pause()).await;

    let section = last_section().expect("the second prompt's modal renders");
    let style = style_of(&section);
    assert!(
        style.contains("calc(-50% + 0px), calc(-50% + 0px)"),
        "the second prompt's modal is centred: {style}"
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
