//! Headless tests for `PromptBanner` (interactivity S3, #538): a live
//! `PickMultiple` prompt renders a bottom-fixed banner whose Confirm submits the
//! toggled selection and whose Pass (when skippable) submits Skip.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::test_support::fixtures::{awaiting_commit_input, awaiting_skippable_commit_input};
use game_core::{InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use std::collections::BTreeSet;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::MultiSelect;
use web::prompt_banner::PromptBanner;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `PromptBanner` with a store carrying `outcome`, a `MultiSelect` whose
/// `selected` starts as `preselected`, and a capturing channel.
async fn mount(
    outcome: game_core::EngineOutcome,
    preselected: &[u32],
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let selected = RwSignal::new(preselected.iter().copied().collect::<BTreeSet<u32>>());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! { <div class="pb-root"><PromptBanner/></div> }
    });
    leptos::task::tick().await;
    rx
}

/// The last-mounted `.pb-root` wrapper — scopes queries to this test's mount so
/// DOM accumulation across tests can't shadow an "absence" assertion.
fn last_root() -> web_sys::Element {
    let roots = document().query_selector_all(".pb-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .pb-root")
}

fn last_banner() -> web_sys::Element {
    last_root()
        .query_selector(".prompt-banner")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .prompt-banner")
}

fn click(sel: &str) {
    last_banner()
        .query_selector(sel)
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("element")
        .click();
}

#[wasm_bindgen_test]
async fn confirm_submits_the_selected_indices() {
    let mut rx = mount(awaiting_commit_input("Commit"), &[0, 2]).await;
    click(".confirm");
    leptos::task::tick().await;
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
        other @ ClientMessage::Submit { .. } => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn confirm_with_no_selection_submits_empty() {
    let mut rx = mount(awaiting_commit_input("Commit"), &[]).await;
    click(".confirm");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickMultiple { selected: vec![] }),
        other @ ClientMessage::Submit { .. } => panic!("expected PickMultiple, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn skippable_prompt_shows_pass_that_submits_skip() {
    let mut rx = mount(awaiting_skippable_commit_input("Commit"), &[]).await;
    click(".pass");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Skip),
        other @ ClientMessage::Submit { .. } => panic!("expected Skip, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn renders_the_prompt_text() {
    let _rx = mount(awaiting_commit_input("Redraw your opening hand"), &[]).await;
    assert!(
        last_banner()
            .text_content()
            .unwrap_or_default()
            .contains("Redraw your opening hand"),
        "banner shows the prompt text"
    );
}

#[wasm_bindgen_test]
async fn no_banner_for_non_skippable_non_multi() {
    // A non-skippable, non-PickMultiple prompt (the encounter-draw Confirm) is not
    // a banner concern — it stays in the flat bar.
    let _rx = mount(
        game_core::test_support::fixtures::awaiting_confirm_input("Draw"),
        &[],
    )
    .await;
    assert!(
        last_root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "banner renders nothing for a non-skippable non-PickMultiple outcome"
    );
}

#[wasm_bindgen_test]
async fn skippable_window_shows_prompt_and_pass_submits_skip() {
    // A skippable PickSingle (reaction/Fast window) → banner with prompt + Pass.
    let outcome =
        game_core::test_support::fixtures::awaiting_skippable_pick_single_input("You may trigger");
    let mut rx = mount(outcome, &[]).await;
    assert!(
        last_banner()
            .text_content()
            .unwrap_or_default()
            .contains("You may trigger"),
        "window prompt shows in the banner"
    );
    click(".pass");
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Skip),
        other @ ClientMessage::Submit { .. } => panic!("expected Skip, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn non_skippable_pick_single_renders_no_banner() {
    // The open turn (non-skippable PickSingle) is not a banner concern.
    let outcome = game_core::test_support::fixtures::awaiting_pick_single_input("Choose an action");
    let _rx = mount(outcome, &[]).await;
    assert!(
        last_root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "no banner for a non-skippable PickSingle"
    );
}
