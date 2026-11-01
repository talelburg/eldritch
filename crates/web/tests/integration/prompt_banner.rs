//! Headless tests for `PromptBanner`.
//!
//! The banner is the **floor**: it names
//! any live prompt and homes whatever has no board surface — un-anchored options,
//! a `PickMultiple` commit's Confirm, a skippable window's Pass, and a fallback
//! Confirm. Its one silence is the open-turn menu, recognised by its
//! `TurnControl` anchor rather than by its text.
#![cfg(target_arch = "wasm32")]

use std::collections::BTreeSet;

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, PickCount,
};
use game_core::state::InvestigatorId;
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::MultiSelect;
use web::prompt_banner::PromptBanner;
use web::store::ClientState;
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement};

/// Mount `PromptBanner` with a store carrying `outcome`, a `MultiSelect` whose
/// `selected` starts as `preselected`, and a capturing channel.
async fn mount(outcome: EngineOutcome, preselected: &[u32]) -> UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let selected = RwSignal::new(preselected.iter().copied().collect::<BTreeSet<u32>>());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! { <div class="pb-root"><PromptBanner/></div> }
    });
    task::tick().await;
    rx
}

/// The last-mounted `.pb-root` wrapper — scopes queries to this test's mount so
/// DOM accumulation across tests can't shadow an "absence" assertion.
fn last_root() -> Element {
    let roots = document().query_selector_all(".pb-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .pb-root")
}

fn last_banner() -> Element {
    last_root()
        .query_selector(".prompt-banner")
        .expect("query")
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .prompt-banner")
}

fn click(sel: &str) {
    last_banner()
        .query_selector(sel)
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("element")
        .click();
}

#[wasm_bindgen_test]
async fn confirm_submits_the_selected_options() {
    let mut rx = mount(
        test_support::awaiting_commit_input("Commit", &[0, 1, 2]),
        &[0, 2],
    )
    .await;
    click(".confirm");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(2)]
            }
        ),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn confirm_with_no_selection_submits_empty() {
    let mut rx = mount(test_support::awaiting_commit_input("Commit", &[0]), &[]).await;
    click(".confirm");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickMultiple { selected: vec![] }),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn skippable_prompt_shows_pass_that_submits_skip() {
    let mut rx = mount(
        test_support::awaiting_skippable_commit_input("Commit", &[0]),
        &[],
    )
    .await;
    click(".pass");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Skip),
        other => panic!("expected Skip, got {other:?}"),
    }
}

/// A hand-size discard that must discard exactly one card, offering two.
fn discard_exactly_one() -> EngineOutcome {
    test_support::awaiting_request(InputRequest::pick_multiple(
        "Choose 1 card to discard",
        vec![
            ChoiceOption::new(OptionId(0), "Flashlight"),
            ChoiceOption::new(OptionId(1), "Knife"),
        ],
        PickCount::exactly(1),
    ))
}

fn confirm_disabled() -> bool {
    last_banner()
        .query_selector(".confirm")
        .expect("query")
        .expect("a Confirm")
        .has_attribute("disabled")
}

/// The engine rejects a discard of the wrong size, so Confirm holds until the
/// selection is one the count admits.
#[wasm_bindgen_test]
async fn confirm_holds_until_the_selection_is_the_count_asked() {
    let mut rx = mount(discard_exactly_one(), &[]).await;
    assert!(confirm_disabled(), "no card chosen of the one asked");
    click(".confirm");
    task::tick().await;
    assert!(rx.try_recv().is_err(), "a disabled Confirm submits nothing");

    let _rx = mount(discard_exactly_one(), &[0, 1]).await;
    assert!(confirm_disabled(), "two cards chosen of the one asked");

    let mut rx = mount(discard_exactly_one(), &[1]).await;
    assert!(!confirm_disabled(), "the one card asked is chosen");
    click(".confirm");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(
            response,
            InputResponse::PickMultiple {
                selected: vec![OptionId(1)]
            }
        ),
        other => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn renders_the_prompt_text() {
    let _rx = mount(
        test_support::awaiting_commit_input("Redraw your opening hand", &[0]),
        &[],
    )
    .await;
    assert!(
        last_banner()
            .text_content()
            .unwrap_or_default()
            .contains("Redraw your opening hand"),
        "banner shows the prompt text"
    );
}

#[wasm_bindgen_test]
async fn an_unanchored_confirm_gets_text_and_a_confirm_button() {
    // The banner is the floor, so a prompt the
    // client does not specifically home still tells the player the engine is
    // waiting — and an un-anchored `Confirm` the result modal is not carrying gets
    // a Confirm here rather than being unreachable.
    let mut rx = mount(
        test_support::awaiting_confirm_input("Something happened"),
        &[],
    )
    .await;
    assert!(last_banner()
        .text_content()
        .unwrap_or_default()
        .contains("Something happened"));
    click(".confirm");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Confirm),
        other => panic!("expected Confirm, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn an_anchored_confirm_gets_text_but_no_banner_confirm() {
    // The Mythos draw's button lives on the encounter deck. The banner still
    // names the prompt — the floor — but must not render a second Confirm, which
    // is the duplication the bar retirement exists to end.
    let mut outcome = test_support::awaiting_confirm_input("Draw");
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::EncounterDeck);
    }
    let _rx = mount(outcome, &[]).await;
    let banner = last_banner();
    assert!(banner.text_content().unwrap_or_default().contains("Draw"));
    assert!(
        banner.query_selector(".confirm").expect("query").is_none(),
        "the encounter deck carries this prompt's only Confirm"
    );
}

#[wasm_bindgen_test]
async fn skippable_window_shows_prompt_and_pass_submits_skip() {
    // A skippable PickSingle (reaction/Fast window) → banner with prompt + Pass.
    let outcome = test_support::awaiting_skippable_pick_single_input("You may trigger");
    let mut rx = mount(outcome, &[]).await;
    assert!(
        last_banner()
            .text_content()
            .unwrap_or_default()
            .contains("You may trigger"),
        "window prompt shows in the banner"
    );
    click(".pass");
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Skip),
        other => panic!("expected Skip, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn the_open_turn_menu_is_suppressed_by_its_anchor() {
    // The one prompt the banner stays silent about. It is identified by the
    // `TurnControl` anchor the engine attaches, never by matching "Choose an
    // action" — the string coupling ADR 0011 exists to prevent. Reserving the one
    // persistent surface means not spending it on every ordinary turn.
    let mut outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "End turn")
            .at(OptionTarget::TurnControl(InvestigatorId(1)))],
    );
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::TurnControl(InvestigatorId(1)));
    }
    let _rx = mount(outcome, &[]).await;
    assert!(
        last_root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "an ordinary turn — every option on a board surface — renders no bar at all"
    );
}

#[wasm_bindgen_test]
async fn the_open_turn_menu_suppresses_its_text_but_not_its_controls() {
    // Only the text. An open-turn action whose own anchor is `None` — Investigate
    // with no current location, EndTurn off-frame, both pinned by engine tests —
    // has no board home, so suppressing the whole banner would make it
    // unreachable rather than merely misplaced (ADR 0011's floor).
    let mut outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![
            ChoiceOption::new(OptionId(0), "End turn")
                .at(OptionTarget::TurnControl(InvestigatorId(1))),
            ChoiceOption::new(OptionId(1), "Investigate"),
        ],
    );
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::TurnControl(InvestigatorId(1)));
    }
    let mut rx = mount(outcome, &[]).await;
    let banner = last_banner();
    assert!(
        banner.query_selector(".prompt").expect("query").is_none(),
        "no 'Choose an action' noise on an ordinary turn"
    );
    let btns = banner.query_selector_all(".banner-option").expect("query");
    assert_eq!(
        btns.length(),
        1,
        "the un-anchored option keeps its only home; the anchored one is on its surface"
    );
    click(".banner-option");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn an_unanchored_pick_single_still_reaches_the_banner() {
    // Not skippable, not multi, no board home: the banner
    // is the only thing between this option and being unreachable. The
    // fixture's two options are un-anchored.
    let outcome = test_support::awaiting_pick_single_input("Choose one");
    let mut rx = mount(outcome, &[]).await;
    let banner = last_banner();
    assert!(banner
        .text_content()
        .unwrap_or_default()
        .contains("Choose one"));
    assert_eq!(
        banner
            .query_selector_all(".banner-option")
            .expect("query")
            .length(),
        2
    );
    click(".banner-option");
    task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

/// Regression for #549: a skippable window's options render as banner buttons
/// (not only Pass), so a Board/Global option such as the round-end advance is
/// reachable.
#[wasm_bindgen_test]
async fn skippable_window_renders_options_that_submit_pick_single() {
    let mut rx = mount(
        test_support::awaiting_skippable_pick_single_input("You may advance"),
        &[],
    )
    .await;
    let btn = last_banner()
        .query_selector(".banner-option")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("an option button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Resolve");
    btn.click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn banner_renders_only_unanchored_options() {
    // With the round-end advance anchored to the act card, the banner does not
    // duplicate anchored options — it renders only un-anchored ones.
    let outcome = test_support::awaiting_skippable_pick_single_with(
        "You may advance",
        vec![
            ChoiceOption::new(OptionId(0), "Advance act").at(OptionTarget::Act),
            ChoiceOption::new(OptionId(1), "Some global"),
        ],
    );
    let mut rx = mount(outcome, &[]).await;
    let banner = last_banner();
    let btns = banner.query_selector_all(".banner-option").expect("query");
    assert_eq!(
        btns.length(),
        1,
        "only the un-anchored option renders as a button"
    );
    let btn = btns
        .item(0)
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("the one banner button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Some global");
    btn.click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other => panic!("expected PickSingle, got {other:?}"),
    }
}
