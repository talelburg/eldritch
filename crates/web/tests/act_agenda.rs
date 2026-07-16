//! Headless render test for the Act/Agenda cards. Own binary so it installs the
//! real `cards::REGISTRY` (registry install is first-wins per process); mounts
//! `act_agenda_view` directly (no investigator panel → no `TEST_INV` lookup).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{Act, Agenda, CardCode, GameStateBuilder};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

fn section_text() -> String {
    let nodes = document()
        .query_selector_all(".act-agenda")
        .expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn act_and_agenda_render_name_text_and_thresholds() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Act 01109 "The Barrier" (Objective text); Agenda 01107 "They're Getting
    // Out!" (Forced text).
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_doom = 1;

    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;

    let text = section_text();
    assert!(text.contains("The Barrier"), "act name missing: {text}");
    assert!(
        text.contains("clues to advance: 2"),
        "act threshold missing: {text}"
    );
    assert!(
        text.contains("Objective"),
        "act ability text missing: {text}"
    );
    assert!(
        text.contains("They're Getting Out!"),
        "agenda name missing: {text}"
    );
    assert!(text.contains("doom 1/3"), "agenda doom missing: {text}");
}

/// The last-mounted `.card--act` — `mount_to_body` accumulates DOM across tests in
/// this binary, so scope to the newest card (the `last_slot`/`last_root` precedent).
fn act_card() -> web_sys::Element {
    let cards = document().query_selector_all(".card--act").expect("query");
    cards
        .item(cards.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card--act")
}

/// Mount `act_agenda_view` (act 01109) with a store carrying `outcome`, a derived
/// `PendingOptions`, an `OutboundTx`, and a capturing channel.
async fn mount_with_prompt(
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_doom = 1;
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        web::act_agenda::act_agenda_view(&state)
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn act_card_glows_and_advances_via_menu() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Advance act",
            OptionTarget::Act,
        )],
    );
    let mut rx = mount_with_prompt(outcome).await;
    let card = act_card();
    assert!(card.class_name().contains("actionable"), "act card glows");
    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Advance act");
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn act_card_inert_without_an_act_anchored_option() {
    // Option anchors Global (not Act) → the act card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "End turn",
            OptionTarget::Global,
        )],
    );
    let _rx = mount_with_prompt(outcome).await;
    assert!(!act_card().class_name().contains("actionable"));
}

/// The last-mounted `.card--agenda` — `mount_to_body` accumulates DOM across tests
/// in this binary, so scope to the newest card (the `act_card` precedent).
fn agenda_card() -> web_sys::Element {
    let cards = document()
        .query_selector_all(".card--agenda")
        .expect("query");
    cards
        .item(cards.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card--agenda")
}

#[wasm_bindgen_test]
async fn agenda_card_glows_and_resolves_via_menu() {
    // An agenda-sourced forced effect anchors its "Resolve" to the agenda card (#556).
    let outcome = awaiting_pick_single_with(
        "Forced — They're Getting Out!",
        vec![ChoiceOption::new(
            OptionId(0),
            "Resolve",
            OptionTarget::Agenda,
        )],
    );
    let mut rx = mount_with_prompt(outcome).await;
    let card = agenda_card();
    assert!(
        card.class_name().contains("actionable"),
        "agenda card glows"
    );
    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Resolve");
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn agenda_card_inert_without_an_agenda_anchored_option() {
    // Option anchors Global (not Agenda) → the agenda card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "End turn",
            OptionTarget::Global,
        )],
    );
    let _rx = mount_with_prompt(outcome).await;
    assert!(!agenda_card().class_name().contains("actionable"));
}

/// Mount `act_agenda_view` with the given deck's leaving card mid-advance at
/// `step`. Pushes an `AdvanceReverse` frame as the engine would (#558).
async fn mount_advancing(
    deck: game_core::state::AdvanceDeck,
    code: &str,
    step: game_core::state::AdvanceStep,
) {
    use game_core::state::{AdvanceDeck, AdvanceTrigger, Continuation};
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = GameStateBuilder::new().build();
    match deck {
        AdvanceDeck::Agenda => {
            state.agenda_deck = vec![Agenda {
                code: CardCode::new(code),
                doom_threshold: 3,
                resolution: None,
            }];
            state.agenda_index = 0;
        }
        AdvanceDeck::Act => {
            state.act_deck = vec![Act {
                code: CardCode::new(code),
                clue_threshold: 2,
                resolution: None,
            }];
            state.act_index = 0;
        }
    }
    state.continuations.push(Continuation::AdvanceReverse {
        deck,
        from: 0,
        leaving_code: CardCode::new(code),
        step,
        trigger: AdvanceTrigger::Forced,
    });
    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;
}

#[wasm_bindgen_test]
async fn agenda_shows_reverse_face_while_advancing() {
    use game_core::state::{AdvanceDeck, AdvanceStep};
    // Once the advance has passed its acknowledge, the agenda flips to its reverse
    // (name + on-advance text) and tags `card--reverse`. `Finalize` is the step the
    // client actually observes — `drive` sets it before firing the reverse, so the
    // reverse renders while 01105's discard-vs-horror ChooseOne is live.
    mount_advancing(AdvanceDeck::Agenda, "01105", AdvanceStep::Finalize).await;
    let text = section_text();
    assert!(
        text.contains("A Lapse in Time"),
        "reverse name shown: {text}"
    );
    assert!(
        text.contains("discard"),
        "reverse text (discard-or-horror choice) shown: {text}"
    );
    assert!(
        !text.contains("What's Going On?!"),
        "front name must NOT show once flipped: {text}"
    );
    assert!(
        agenda_card().class_name().contains("card--reverse"),
        "the flipped agenda is tagged card--reverse"
    );
    assert!(
        !text.contains("doom"),
        "the doom track belongs to the front face, not the reverse: {text}"
    );
}

#[wasm_bindgen_test]
async fn agenda_shows_front_face_before_the_flip() {
    use game_core::state::{AdvanceDeck, AdvanceStep};
    // Before the flip is clicked (step AwaitAck), the front face is still shown, the
    // card is not tagged reverse, and the doom track is present.
    mount_advancing(AdvanceDeck::Agenda, "01105", AdvanceStep::AwaitAck).await;
    let text = section_text();
    assert!(
        text.contains("What's Going On?!"),
        "front name shown pre-flip: {text}"
    );
    assert!(
        !text.contains("A Lapse in Time"),
        "reverse name must NOT show pre-flip: {text}"
    );
    assert!(
        !agenda_card().class_name().contains("card--reverse"),
        "pre-flip agenda is NOT tagged card--reverse"
    );
    assert!(
        text.contains("doom 0/3"),
        "the front face shows the doom track: {text}"
    );
}

#[wasm_bindgen_test]
async fn act_shows_reverse_face_while_advancing() {
    use game_core::state::{AdvanceDeck, AdvanceStep};
    // The act side flips too: act 01109 "The Barrier" → reverse "Breaking the
    // Barrier" (back_text reveals the Parlor).
    mount_advancing(AdvanceDeck::Act, "01109", AdvanceStep::Finalize).await;
    let text = section_text();
    assert!(
        text.contains("Breaking the Barrier"),
        "act reverse name shown: {text}"
    );
    assert!(text.contains("Parlor"), "act reverse text shown: {text}");
    assert!(
        act_card().class_name().contains("card--reverse"),
        "the flipped act is tagged card--reverse"
    );
    assert!(
        !text.contains("clues to advance"),
        "the advance-cost line belongs to the front face, not the reverse: {text}"
    );
}
