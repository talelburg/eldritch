//! Headless render tests for `BoardView` (P6.5). Feed a constructed
//! `GameState` through the store and assert on the rendered DOM.
//! wasm32-only (browser DOM); native jobs skip this file.
//!
//! **This binary runs against the real `cards::REGISTRY` (#868).** It used to
//! install `game_core::test_support::install_test_registry()`, which knows only
//! `TEST_INV` and the synthetic terminal act/agenda cards — so every card code
//! the file seeded failed to resolve and the assertions passed on the
//! renderer's unresolved-code fallback (`card--unknown`) rather than on card
//! rendering. Real codes make that structural: nothing here *can* ride the
//! fallback, and [`render_state`] asserts as much on every render.
//!
//! Two of the three code→name paths the board can take are pinned here: the
//! `Card` component's (hand / in play / threat area) and
//! `act_agenda::name_and_text_src`'s. The third — `names::card_name`, reached
//! only from the map's card-at-location token (`crates/web/src/map.rs`) — is
//! *not*, because this file seeds no `cards_at_location` and no location
//! attachments. `crates/web/tests/card_at_location.rs` and the unit tests in
//! `crates/web/src/names.rs` own that one.
//!
//! Locations and enemies stay on `game_core::test_support::fixtures` — per
//! ADR 0016 those are primitive builders, not card impersonations, and both
//! carry their name and stats in `GameState` rather than reading the registry.
#![cfg(target_arch = "wasm32")]

use game_core::state::GameStateBuilder;
use game_core::state::{Act, Agenda, CardCode, Investigator};
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

/// Roland Banks — 9 health / 5 sanity, per
/// `data/arkhamdb-snapshot/pack/core/core.json`.
const ROLAND: &str = "01001";
/// Roland's .38 Special — his signature asset (`deck_requirements` on 01001
/// names `card:01006`). Item. Weapon. Firearm., cost 3, Hand slot.
const ROLANDS_38_SPECIAL: &str = "01006";
/// Magnifying Glass — Seeker level 0, which Roland's `deck_options` admit
/// (`{"faction":["seeker"],"level":{"min":0,"max":2}}`), and `deck_limit: 2`,
/// so the copy in play and the copy in hand are both legal.
const MAGNIFYING_GLASS: &str = "01030";
/// Cover Up — Roland's signature weakness (`card:01007`). Its **Revelation**
/// reads *"Put Cover Up into play in your threat area, with 3 clues on it."*,
/// which is why it models a threat-area card and never a card in hand: it is a
/// treachery weakness, i.e. an encounter cardtype, and
/// `data/rules-reference/rules/glossary/Revelation.md` says *"When a weakness
/// card enters an investigator's hand, that investigator must immediately
/// resolve all revelation abilities on the card as if it were just drawn."*
const COVER_UP: &str = "01007";
/// The Gathering act 1, "Trapped" — printed `clues: 2`.
const ACT_TRAPPED: &str = "01108";
/// The Gathering agenda 1, "What's Going On?!" — printed `doom: 3`.
const AGENDA_WHATS_GOING_ON: &str = "01105";

fn body_html() -> String {
    leptos::prelude::document()
        .body()
        .expect("body")
        .inner_html()
}

/// `test_investigator(id)` carrying Roland Banks as its investigator card.
///
/// Every investigator this file renders needs a code the real registry knows:
/// the panel reads `max_health()` / `max_sanity()`, and `investigator_capacity`
/// panics on a code the registry cannot resolve rather than defaulting
/// (`crates/game-core/src/state/investigator.rs:211-220`). The fixture's own
/// `TEST_INV` is absent from the corpus, so it is replaced here rather than at
/// each of the six call sites.
fn roland(id: u32) -> Investigator {
    let mut inv = test_investigator(id);
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv
}

/// The last mounted element matching `sel` (DOM accumulates across tests on the
/// shared page — scope to the latest subtree).
fn last_mounted(sel: &str) -> web_sys::Element {
    let nodes = leptos::prelude::document()
        .query_selector_all(sel)
        .expect("query_selector_all");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .unwrap_or_else(|| panic!("at least one {sel}"))
}

/// Visible text of `el`'s descendant matching `sel`.
///
/// Names are asserted against `text_content` rather than `inner_html` because
/// the latter escapes — the apostrophe in "What's Going On?!" arrives as an
/// entity, and a substring test against it would be testing the escaper.
fn text_of(el: &web_sys::Element, sel: &str) -> String {
    el.query_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("expected {sel}"))
        .text_content()
        .unwrap_or_default()
}

/// How many descendants of `el` match `sel`.
fn count_in(el: &web_sys::Element, sel: &str) -> u32 {
    el.query_selector_all(sel).expect("query").length()
}

/// Mount `BoardView` against a fresh store and feed it one `Hello`
/// carrying `state`. Ticks once so CSR effects flush, then returns the
/// rendered body HTML.
async fn render_state(state: game_core::state::GameState) -> String {
    // The real corpus registry: the code→name/kind source, and the source of
    // the investigator capacity the panel reads (#448). Idempotent (OnceLock,
    // first-wins); `web` has no `ctor` dev-dep, so install in-test.
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
    let html = body_html();
    // #868: the guard that keeps this file honest. `Card` renders
    // `card--unknown` for a code the registry cannot resolve
    // (`crates/web/src/card.rs:324-331`), and that element still matches
    // `.card` — so a selector or substring assertion below could pass on the
    // fallback without anyone noticing, which is exactly how this file rotted.
    // Every code it seeds is a real one, so the class must never appear.
    assert!(
        !html.contains("card--unknown"),
        "a seeded code failed to resolve against cards::REGISTRY: {html}"
    );
    html
}

#[wasm_bindgen_test]
async fn act_agenda_cards_render_name_and_thresholds() {
    let mut state = GameStateBuilder::new().with_investigator(roland(1)).build();
    state.act_deck = vec![Act {
        code: CardCode::new(ACT_TRAPPED),
        clue_threshold: 2,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA_WHATS_GOING_ON),
        doom_threshold: 3,
    }];
    state.agenda_doom = 1;

    let html = render_state(state).await;

    // Names come from the corpus via `act_agenda::name_and_text_src`, which
    // `crates/web/tests/act_agenda.rs` already covers in more depth (including
    // the reverse `back_name` face). These two assertions are here so the test
    // earns its own name: it asserted only thresholds before (#868), which is
    // how the file came to advertise coverage it did not have.
    let board = last_mounted(".board");
    let text = board.text_content().unwrap_or_default();
    assert!(text.contains("Trapped"), "act name missing: {text}");
    assert!(
        text.contains("What's Going On?!"),
        "agenda name missing: {text}"
    );

    assert!(html.contains("doom 1/3"), "agenda doom missing: {html}");
    assert!(
        html.contains("clues to advance: 2"),
        "act threshold missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn map_renders_location_name_shroud_clues() {
    let mut loc = test_location(7, "Rivertown");
    loc.shroud = 3;
    loc.clues = 2;
    let state = GameStateBuilder::new()
        .with_investigator(roland(1))
        .with_location(loc)
        .build();

    let html = render_state(state).await;

    assert!(html.contains("Rivertown"), "location name missing: {html}");
    // The header prints numerals in fixed, colour-coded slots rather than
    // labels (#848); the words survive only as the badges' `title`.
    assert!(
        html.contains(r#"title="shroud 3""#),
        "shroud badge missing: {html}"
    );
    assert!(
        html.contains(r#"title="clues 2""#),
        "clue badge missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn investigators_panel_renders_stats_and_hand() {
    use game_core::state::{CardInPlay, CardInstanceId, Skills};

    let mut inv = roland(1);
    inv.name = "Roland Banks".to_string();
    inv.skills = Skills {
        willpower: 5,
        intellect: 4,
        combat: 3,
        agility: 2,
    };
    inv.investigator_card.accumulated_damage = 2; // hp 2/9
    inv.investigator_card.accumulated_horror = 1; // san 1/5
    inv.clues = 3;
    inv.resources = 4;
    inv.actions_remaining = 2;
    inv.hand = vec![
        CardCode::new(ROLANDS_38_SPECIAL),
        CardCode::new(MAGNIFYING_GLASS),
    ];
    inv.cards_in_play = vec![CardInPlay::enter_play(
        CardCode::new(MAGNIFYING_GLASS),
        CardInstanceId(0),
    )];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;

    // Every assertion is scoped to this test's own panel. DOM accumulates
    // across tests on the shared page, and now that real card *text* renders
    // too, a bare `html.contains(..)` shares the page with the printed text of
    // four cards — a false pass from a collision is the same failure mode #868
    // is about.
    let inv_el = last_mounted(".investigator");

    // Identity + folded vitals (skills + hp/san) live in the investigator block.
    assert_eq!(text_of(&inv_el, ".inv-name"), "Roland Banks");
    assert_eq!(text_of(&inv_el, ".inv-skills"), "W5 I4 C3 A2");
    assert_eq!(text_of(&inv_el, ".inv-hp"), "hp 2/9");
    assert_eq!(text_of(&inv_el, ".inv-san"), "san 1/5");
    // Meta cluster: actions as pips.
    assert_eq!(
        count_in(&inv_el, ".inv-meta .inv-actions .pip"),
        2,
        "two action pips: {html}"
    );
    assert!(
        text_of(&inv_el, ".inv-resources").contains("resources 4"),
        "resources missing: {html}"
    );
    assert_eq!(text_of(&inv_el, ".inv-clues"), "clues 3");
    // Location is no longer shown in the panel (it's on the map token).
    assert!(
        !html.contains("inv-location"),
        "location line should be gone: {html}"
    );

    // Cards render, in the zone they were seeded into. Magnifying Glass sits in
    // both hand and play (`deck_limit: 2`), so a page-wide name check could not
    // tell the zones apart — the counts and the per-zone text are what make
    // these assertions load-bearing.
    assert_eq!(
        count_in(&inv_el, ".inv-zones-bottom .hand .card"),
        2,
        "two hand cards: {html}"
    );
    let hand = text_of(&inv_el, ".inv-zones-bottom .hand");
    assert!(
        hand.contains("Roland's .38 Special"),
        "hand card name missing: {hand}"
    );
    assert!(
        hand.contains("Magnifying Glass"),
        "hand card name missing: {hand}"
    );
    assert_eq!(
        count_in(&inv_el, ".inv-zones-top .in-play .card"),
        1,
        "one in-play card: {html}"
    );
    assert!(
        text_of(&inv_el, ".inv-zones-top .in-play").contains("Magnifying Glass"),
        "in-play card name missing: {html}"
    );

    // Layout: in-play + threat in the top row; investigator block (card + vitals +
    // meta) and hand in the bottom row.
    for sel in [
        ".inv-zones-top .in-play",
        ".inv-zones-top .threat",
        ".inv-zones-bottom .investigator-block .investigator-card .card-slot",
        ".inv-zones-bottom .investigator-block .inv-vitals",
        ".inv-zones-bottom .investigator-block .inv-meta",
        ".inv-zones-bottom .hand",
    ] {
        assert!(
            inv_el.query_selector(sel).expect("query ok").is_some(),
            "expected layout element {sel}: {html}"
        );
    }
}

#[wasm_bindgen_test]
async fn empty_board_renders_placeholder_without_panels() {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    leptos::task::tick().await;

    // Scope to only the last mounted <section class="board"> so that
    // accumulated DOM from earlier tests does not pollute this assertion.
    let html = last_mounted(".board").inner_html();

    assert!(
        html.contains("&lt;no game&gt;"),
        "placeholder missing: {html}"
    );
    assert!(
        !html.contains("Investigators"),
        "panels should be absent: {html}"
    );
    assert!(
        !html.contains("Locations"),
        "panels should be absent: {html}"
    );
}

#[wasm_bindgen_test]
async fn resolution_banner_names_the_resolution_point() {
    use game_core::{ResolutionId, ScenarioEnding};
    let mut state = GameStateBuilder::new().with_investigator(roland(1)).build();
    state.ending = Some(ScenarioEnding::Resolution(ResolutionId::new(3)));

    let _ = render_state(state).await;

    // The banner names the ending the player takes to the campaign guide, not
    // a win/loss verdict — R3 is agenda-invoked in The Gathering, and calling
    // that "lost" is the standalone-mode projection this client no longer
    // makes.
    let html = last_mounted(".resolution").inner_html();
    assert!(
        html.contains("Resolution 3"),
        "banner must name the resolution point: {html}"
    );
    assert!(
        !html.contains("won") && !html.contains("lost"),
        "banner must not adjudicate win/loss: {html}"
    );
}

#[wasm_bindgen_test]
async fn resolution_banner_renders_no_resolution_reached() {
    use game_core::ScenarioEnding;
    let mut state = GameStateBuilder::new().with_investigator(roland(1)).build();
    state.ending = Some(ScenarioEnding::NoResolution);

    let _ = render_state(state).await;

    // RR Elimination step 6's ending has its own campaign-guide entry ("If no
    // resolution was reached"), and an investigator who got here by resigning
    // is "not considered to have been defeated".
    let html = last_mounted(".resolution").inner_html();
    assert!(
        html.contains("no resolution reached"),
        "no-resolution banner text missing: {html}"
    );
    assert!(
        !html.contains("lost"),
        "no resolution reached is not a loss: {html}"
    );
}

#[wasm_bindgen_test]
async fn map_and_investigators_are_inside_board_main() {
    let state = GameStateBuilder::new()
        .with_investigator(roland(1))
        .with_location(test_location(1, "Study"))
        .build();
    let _ = render_state(state).await;

    // Scope to the last mounted .game so DOM accumulation from earlier tests
    // does not pollute this assertion.
    let last_game = last_mounted(".game");

    assert!(
        last_game
            .query_selector(".board-main .map")
            .expect("query ok")
            .is_some(),
        ".map must be a descendant of .board-main"
    );
    assert!(
        last_game
            .query_selector(".board-main .investigators")
            .expect("query ok")
            .is_some(),
        ".investigators must be a descendant of .board-main"
    );
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_as_card_in_threat_area() {
    use game_core::state::InvestigatorId;
    use game_core::test_support::fixtures::test_enemy;

    // `test_enemy` is a primitive builder (ADR 0016): the `Enemy` struct carries
    // its own name, traits and stats, and `EnemyCard` reads them from state —
    // the registry is consulted only for optional ability text
    // (`crates/web/src/enemy_card.rs:54-63`), and never renders `card--unknown`.
    let mut enemy = test_enemy(1, "Ghoul Priest");
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_investigator(roland(1))
        .with_enemy(enemy)
        .build();

    let html = render_state(state).await;

    let last_game = last_mounted(".game");
    let card = last_game
        .query_selector(".threat .card-row .card")
        .expect("query_selector");
    assert!(
        card.is_some(),
        "engaged enemy should render as a card: {html}"
    );
    assert!(html.contains("Ghoul Priest"), "enemy name missing: {html}");
}

#[wasm_bindgen_test]
async fn threat_area_treachery_renders_as_card() {
    use game_core::state::{CardInPlay, CardInstanceId};

    let mut inv = roland(1);
    // Seeded the way its Revelation puts it into play — "with 3 clues on it" —
    // so the state matches the card rather than merely occupying the zone.
    let mut cover_up = CardInPlay::enter_play(CardCode::new(COVER_UP), CardInstanceId(0));
    cover_up.clues = 3;
    inv.threat_area = vec![cover_up];
    let state = GameStateBuilder::new().with_investigator(inv).build();

    let html = render_state(state).await;

    let last_game = last_mounted(".game");
    // A treachery has no bespoke face — `card_face` returns `None` for it, so
    // it renders the generic rectangle (`crates/web/src/card.rs:416-433`).
    // Asserting `card--generic` rather than the bare `.card` is what
    // distinguishes it from `card--unknown`, which also matches `.card` and is
    // what this assertion used to be silently passing on (#868).
    let card = last_game
        .query_selector(".threat .card-row .card--generic")
        .expect("query_selector");
    assert!(
        card.is_some(),
        "threat-area treachery should render as a card: {html}"
    );
    let treachery = text_of(&last_game, ".threat .card-row .card--generic");
    assert!(
        treachery.contains("Cover Up"),
        "treachery name missing: {html}"
    );
    assert!(
        treachery.contains("clues 3"),
        "clues-on-card chip missing: {html}"
    );
}
