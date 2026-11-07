//! Headless render tests for the `Card` component.
//!
//! wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use std::collections::BTreeSet;

use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Owner as CardOwner};
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::{self, Card, HandCardView};
use web::glyph;
use web::interaction::{MultiSelect, PendingOptions};
use web::registry::provide_cards;
use web::store::ClientState;
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement};

use crate::common;

/// Inner HTML of the last mounted `.card` (DOM accumulates across tests on the
/// shared page — scope to the latest subtree).
fn last_card_html() -> String {
    let cards = document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_ref::<Element>()
        .expect("Element")
        .inner_html()
}

async fn mount_card(code: &str) -> String {
    let code = CardCode::new(code);
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <Card code=code.clone()/> }
    });
    task::tick().await;
    last_card_html()
}

#[wasm_bindgen_test]
async fn asset_renders_cost_name_traits_text_icons() {
    // Machete 01020: Guardian, cost 3, Hand slot, 1 combat icon, text with
    // [action], <b>Fight.</b>, and [combat].
    let html = mount_card("01020").await;
    assert!(html.contains("Machete"), "name missing: {html}");
    assert!(html.contains('3'), "cost missing: {html}");
    assert!(html.contains("Weapon"), "traits missing: {html}");
    assert!(html.contains("Fight."), "bold text missing: {html}");
    // `[action]` and `[combat]` in the text become glyphs. Scoped to
    // `.card-text`, since the footer's skill chip also names combat.
    let text = common::one(&common::last_mounted(".card"), ".card-text");
    let glyph = |token: &str| {
        let span = common::one(&text, &format!(".glyph.glyph--{token}"));
        span.text_content().unwrap_or_default()
    };
    assert_eq!(glyph("action"), "t", "action glyph in the card text");
    assert_eq!(glyph("combat"), "h", "combat glyph in the card text");
    // The footer's skill icon is the same glyph-in-colour-circle as the tile's,
    // so a tile and its popover agree.
    let footer = common::one(&common::last_mounted(".card"), ".card-footer");
    let circle = common::one(&footer, ".skill-icon.skill-icon--combat");
    assert_eq!(circle.get_attribute("title").as_deref(), Some("combat"));
    assert_eq!(
        common::text_of(&circle, ".glyph.glyph--combat"),
        "h",
        "combat glyph in the footer"
    );
}

/// Every token the glyph table maps, its codepoint and its readable name,
/// copied from `arkhamicons-config.json` rather than from the table, so a
/// codepoint typo in the table fails here.
const EXPECTED_GLYPHS: &[(&str, &str, &str)] = &[
    ("willpower", "f", "willpower"),
    ("intellect", "g", "intellect"),
    ("combat", "h", "combat"),
    ("agility", "i", "agility"),
    ("wild", "j", "wild"),
    ("action", "t", "action"),
    ("reaction", "u", "reaction"),
    ("fast", "v", "free triggered ability"),
    ("free", "v", "free triggered ability"),
    ("elder_sign", "k", "elder sign"),
    ("skull", "m", "skull"),
    ("cultist", "n", "cultist"),
    ("tablet", "o", "tablet"),
    ("elder_thing", "p", "elder thing"),
    ("auto_fail", "q", "auto-fail"),
    ("per_investigator", "r", "per investigator"),
    ("guardian", ")", "guardian"),
    ("seeker", "b", "seeker"),
    ("mystic", "c", "mystic"),
    ("rogue", "d", "rogue"),
    ("survivor", "e", "survivor"),
    ("curse", "y", "curse"),
    ("bless", "z", "bless"),
    ("codex", "\u{e901}", "codex"),
];

#[wasm_bindgen_test]
async fn every_mapped_token_renders_its_glyph_in_card_text() {
    let mapped: BTreeSet<&str> = glyph::GLYPHS.iter().map(|g| g.token).collect();
    let expected: BTreeSet<&str> = EXPECTED_GLYPHS.iter().map(|(t, _, _)| *t).collect();
    assert_eq!(
        mapped, expected,
        "the table maps exactly the expected tokens"
    );

    for &(token, codepoint, name) in EXPECTED_GLYPHS {
        let markup = format!("x [{token}] y");
        let (_, container) = common::mount(move || {
            view! { <div class="card-text">{card::render_segments(card::parse_card_text(&markup))}</div> }
        });
        task::tick().await;
        let span = common::one(&container, ".glyph");
        assert_eq!(
            span.class_name(),
            format!("glyph glyph--{token}"),
            "[{token}] class"
        );
        assert_eq!(
            span.text_content().unwrap_or_default(),
            codepoint,
            "[{token}] codepoint"
        );
        assert_eq!(
            span.get_attribute("aria-label").as_deref(),
            Some(name),
            "[{token}] label"
        );
        assert_eq!(
            span.get_attribute("title").as_deref(),
            Some(name),
            "[{token}] title"
        );
    }
}

#[wasm_bindgen_test]
async fn unmapped_token_stays_a_red_unknown_token() {
    let (_, container) = common::mount(|| {
        view! { <div class="card-text">{card::render_segments(card::parse_card_text("[mystery]"))}</div> }
    });
    task::tick().await;
    assert_eq!(common::text_of(&container, ".unknown-token"), "[mystery]");
}

#[wasm_bindgen_test]
async fn guardian_card_carries_class_modifier() {
    let _ = mount_card("01020").await;
    // Scope to the last mounted .card (DOM accumulates across tests on the
    // shared page) and assert IT carries the guardian class modifier.
    let cards = document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    let last = cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<Element>()
        .expect("Element");
    assert!(
        last.class_list().contains("card--guardian"),
        "last mounted card should carry the guardian class modifier"
    );
}

/// The unresolved-code fallback is **intended** behaviour, not an accident: a
/// code the registry cannot resolve (an unimplemented stub) renders as a bare
/// `card--unknown` rectangle
/// showing the raw code, rather than as nothing.
///
/// The class is asserted, not just the code, because it is the contract
/// `crates/web/tests/integration/board.rs` reads: that file asserts `card--unknown` never
/// appears, which is only a real guard if the fallback is the sole producer of
/// it. `entity_names.rs` holds the complementary property — that a code
/// which *does* resolve never leaks to the player.
#[wasm_bindgen_test]
async fn unknown_code_falls_back_to_raw_code() {
    let html = mount_card("99999").await;
    assert!(html.contains("99999"), "raw code fallback missing: {html}");
    assert!(
        last_card_classes().contains("card--unknown"),
        "unresolved code should use the unknown arm: {}",
        last_card_classes()
    );
}

/// Class list of the last mounted `.card` element.
fn last_card_classes() -> String {
    let cards = document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<Element>()
        .expect("Element")
        .class_name()
}

#[wasm_bindgen_test]
async fn in_play_exhausted_asset_dims_badges_and_shows_soak() {
    // Beat Cop 01018: ally asset, health 2 / sanity 2.
    let mut inst = CardInPlay::enter_play(
        CardCode::new("01018"),
        CardInstanceId(0),
        CardOwner::Investigator(InvestigatorId(1)),
    );
    inst.exhausted = true;
    inst.accumulated_damage = 1;
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> }
    });
    task::tick().await;

    assert!(
        last_card_classes().contains("card--exhausted"),
        "exhausted class missing"
    );
    let html = last_card_html();
    assert!(
        html.contains("Exhausted"),
        "exhausted badge missing: {html}"
    );
    assert!(html.contains("dmg 1/2"), "soak chip missing: {html}");
    assert!(
        !html.contains("card-cost"),
        "in-play card must not show a cost corner: {html}"
    );
}

#[wasm_bindgen_test]
async fn treachery_renders_generic_face_with_clues() {
    // Cover Up 01007: treachery/weakness, traits "Task.", Revelation text;
    // enters the threat area with clues on the card.
    let mut inst = CardInPlay::enter_play(
        CardCode::new("01007"),
        CardInstanceId(0),
        CardOwner::Investigator(InvestigatorId(1)),
    );
    inst.clues = 3;
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <Card code=CardCode::new("01007") in_play=inst.clone()/> }
    });
    task::tick().await;

    assert!(
        last_card_classes().contains("card--generic"),
        "treachery should use the generic arm"
    );
    let html = last_card_html();
    assert!(html.contains("Cover Up"), "name missing: {html}");
    assert!(html.contains("Task"), "trait missing: {html}");
    assert!(
        html.contains("clues 3"),
        "clues-on-card chip missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn in_play_ready_asset_is_not_dimmed() {
    let inst = CardInPlay::enter_play(
        CardCode::new("01018"),
        CardInstanceId(0),
        CardOwner::Investigator(InvestigatorId(1)),
    );
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> }
    });
    task::tick().await;
    assert!(
        !last_card_classes().contains("card--exhausted"),
        "ready card must not be dimmed"
    );
    assert!(
        !last_card_html().contains("card-cost"),
        "in-play card must not show a cost corner regardless of exhaustion"
    );
}

/// The last-mounted `.hand-slot`.
fn last_slot() -> Element {
    let slots = document().query_selector_all(".hand-slot").expect("query");
    slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .hand-slot")
}

/// Mount a `HandCardView` (Machete 01020, investigator 1, index 0) with a store
/// carrying `outcome`, `PendingOptions` derived from it, a `MultiSelect` whose
/// `active` reflects the outcome, and a capturing outbound channel.
async fn mount_hand(
    outcome: EngineOutcome,
) -> (RwSignal<BTreeSet<u32>>, UnboundedReceiver<ClientMessage>) {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| {
        s.outcome = Some(outcome);
        common::you_hold_every_investigator(s);
    });
    let selected = RwSignal::new(BTreeSet::<u32>::new());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! {
            <HandCardView code=CardCode::new("01020") investigator=InvestigatorId(1) index=0/>
        }
    });
    task::tick().await;
    (selected, rx)
}

#[wasm_bindgen_test]
async fn playable_hand_card_opens_a_play_menu_and_submits() {
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![
            ChoiceOption::new(OptionId(0), "Play Machete").at(OptionTarget::HandCard {
                investigator: InvestigatorId(1),
                hand_index: 0,
            }),
        ],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;

    let slot = last_slot();
    assert!(slot.class_name().contains("actionable"), "slot glows");
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    task::tick().await;
    let item = slot
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Play Machete");
    item.click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected ResolveInput, got {other:?}"),
    }
}

/// The mounted card (investigator 1's hand index 0) is offered as option 1, so
/// a click selects option 1 — the option's id, not the hand index.
#[wasm_bindgen_test]
async fn an_offered_hand_card_toggles_its_option_in_select_mode() {
    let (selected, _rx) =
        mount_hand(test_support::awaiting_commit_input("Commit cards", &[3, 0])).await;
    let slot = last_slot();
    assert!(
        !slot.class_name().contains("actionable"),
        "no Play menu in select mode"
    );
    assert!(
        slot.class_name().contains("selectable"),
        "the card is offered"
    );
    slot.clone()
        .dyn_into::<HtmlElement>()
        .expect("HtmlElement")
        .click();
    task::tick().await;
    assert_eq!(
        selected.get_untracked(),
        BTreeSet::from([1]),
        "the card's option is selected"
    );
    assert!(
        last_slot().class_name().contains("selected"),
        "selected ring shown"
    );
}

/// A hand card the pick does not offer stays inert in select mode: no ring to
/// toggle, nothing selected on a click.
#[wasm_bindgen_test]
async fn a_hand_card_the_pick_does_not_offer_is_inert() {
    let (selected, _rx) =
        mount_hand(test_support::awaiting_commit_input("Commit cards", &[2])).await;
    let slot = last_slot();
    assert!(!slot.class_name().contains("selectable"), "not offered");
    slot.clone()
        .dyn_into::<HtmlElement>()
        .expect("HtmlElement")
        .click();
    task::tick().await;
    assert!(selected.get_untracked().is_empty(), "nothing selected");
}

#[wasm_bindgen_test]
async fn hand_card_glows_for_a_reaction_anchored_by_code() {
    // A HandCardByCode-anchored option (a Fast reaction event) glows the hand
    // card of that code (Machete 01020 as a stand-in) and opens its menu.
    let outcome = test_support::awaiting_pick_single_with(
        "You may play a card",
        vec![ChoiceOption::new(OptionId(0), "Play Machete from hand").at(
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new("01020"),
            },
        )],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;
    let slot = last_slot();
    assert!(
        slot.class_name().contains("actionable"),
        "reaction card glows by code"
    );
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    task::tick().await;
    slot.query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a menu item")
        .click();
    task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected ResolveInput, got {other:?}"),
    }
}
