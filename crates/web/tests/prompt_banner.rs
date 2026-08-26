//! Headless tests for `PromptBanner` (interactivity S3/S6, #538/#541).
//!
//! Since the flat `.action-bar` was deleted the banner is the **floor**: it names
//! any live prompt and homes whatever has no board surface — un-anchored options,
//! a `PickMultiple` commit's Confirm, a skippable window's Pass, and a fallback
//! Confirm. Its one silence is the open-turn menu, recognised by its
//! `TurnControl` anchor rather than by its text.
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
async fn an_unanchored_confirm_gets_text_and_a_confirm_button() {
    // S6 (#541): with the flat bar deleted the banner is the floor, so a prompt the
    // client does not specifically home still tells the player the engine is
    // waiting — and an un-anchored `Confirm` the result modal is not carrying gets
    // a Confirm here rather than being unreachable.
    let mut rx = mount(
        game_core::test_support::fixtures::awaiting_confirm_input("Something happened"),
        &[],
    )
    .await;
    assert!(last_banner()
        .text_content()
        .unwrap_or_default()
        .contains("Something happened"));
    click(".confirm");
    leptos::task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Confirm),
        other @ ClientMessage::Submit { .. } => panic!("expected Confirm, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn an_anchored_confirm_gets_text_but_no_banner_confirm() {
    // The Mythos draw's button lives on the encounter deck. The banner still
    // names the prompt — the floor — but must not render a second Confirm, which
    // is the duplication the bar retirement exists to end.
    use game_core::{EngineOutcome, OptionTarget};
    let mut outcome = game_core::test_support::fixtures::awaiting_confirm_input("Draw");
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
async fn the_open_turn_menu_is_suppressed_by_its_anchor() {
    // The one prompt the banner stays silent about. It is identified by the
    // `TurnControl` anchor the engine attaches, never by matching "Choose an
    // action" — the string coupling ADR 0011 exists to prevent. Reserving the one
    // persistent surface means not spending it on every ordinary turn.
    use game_core::state::InvestigatorId;
    use game_core::{EngineOutcome, OptionTarget};
    let mut outcome =
        game_core::test_support::fixtures::awaiting_pick_single_input("Choose an action");
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::TurnControl(InvestigatorId(1)));
    }
    let _rx = mount(outcome, &[]).await;
    assert!(
        last_root()
            .query_selector(".prompt-banner")
            .expect("query")
            .is_none(),
        "no banner for the open-turn menu"
    );
}

#[wasm_bindgen_test]
async fn an_unanchored_pick_single_still_reaches_the_banner() {
    // Not skippable, not multi, no board home: with the flat bar gone the banner
    // is the only thing between this option and being unreachable (#541). The
    // fixture's two options are un-anchored.
    let outcome = game_core::test_support::fixtures::awaiting_pick_single_input("Choose one");
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
    leptos::task::tick().await;
    match rx.try_recv().expect("a frame after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn skippable_window_renders_options_that_submit_pick_single() {
    // The round-end-advance fix (#549): a skippable window's options render as
    // banner buttons (not only Pass), so a Board/Global option is reachable.
    let mut rx = mount(
        game_core::test_support::fixtures::awaiting_skippable_pick_single_input("You may advance"),
        &[],
    )
    .await;
    let btn = last_banner()
        .query_selector(".banner-option")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("an option button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Resolve");
    btn.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected PickSingle, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn banner_renders_only_unanchored_options() {
    // S5 (#540): once the round-end advance is anchored to the act card, the banner
    // stops duplicating anchored options — it renders only un-anchored ones.
    use game_core::test_support::fixtures::awaiting_skippable_pick_single_with;
    use game_core::{ChoiceOption, OptionTarget};
    let outcome = awaiting_skippable_pick_single_with(
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
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("the one banner button");
    assert_eq!(btn.text_content().unwrap_or_default(), "Some global");
    btn.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(1))),
        other @ ClientMessage::Submit { .. } => panic!("expected PickSingle, got {other:?}"),
    }
}
