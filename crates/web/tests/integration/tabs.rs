//! The tabs over the dock: one tab per investigator, in seating order, and the
//! open tab's investigator docked below them.
//!
//! Each game is [`mount_table`]'s: Tal, [`YOU`], holds Roland Banks and Agnes
//! Baker, Sam holds Daisy Walker, and Alex is a viewer.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{location, seat};
use leptos::task;
use protocol::lobby::Member;
use protocol::{AccountId, Assignment, GameId, LogPosition, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::store::{self, ClientState};
use web_sys::{Element, HtmlElement};

use crate::common::{self, BoardHandle, YOU};
use crate::lobby::{self, SAM};

/// Alex, a member holding no investigator.
pub(crate) const ALEX: AccountId = AccountId(3);
/// Roland Banks, Tal's, the lead, seated first.
pub(crate) const ROLAND: InvestigatorId = InvestigatorId(1);
/// Agnes Baker, Tal's, seated third.
pub(crate) const AGNES: InvestigatorId = InvestigatorId(2);
/// Daisy Walker, Sam's, seated second.
pub(crate) const DAISY: InvestigatorId = InvestigatorId(3);
/// The Study 01111, where everyone stands.
const STUDY: LocationId = LocationId(10);

/// Roland's hand.
pub(crate) const ROLANDS_HAND: [&str; 2] = ["01020", "01021"];
/// Daisy's hand.
pub(crate) const DAISYS_HAND: [&str; 2] = ["01030", "01031"];
/// Agnes's hand, the largest.
pub(crate) const AGNESS_HAND: [&str; 4] = ["01060", "01061", "01062", "01063"];

/// Tal, Sam and Alex, in the order they joined.
fn members() -> Vec<Member> {
    [(YOU, "Tal"), (SAM, "Sam"), (ALEX, "Alex")]
        .into_iter()
        .map(|(account, name)| Member {
            account,
            display_name: name.to_string(),
        })
        .collect()
}

/// The table's game, for a caller to stage further: Roland, Daisy and Agnes
/// seated in that order, which is not their id order, each holding a hand, in
/// the Study.
pub(crate) fn table() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_location(location(STUDY).as_card("01111"))
        .with_seat(seat(ROLAND.0).as_card("01001").at(STUDY).hand(ROLANDS_HAND))
        .with_seat(seat(AGNES.0).as_card("01004").at(STUDY).hand(AGNESS_HAND))
        .with_seat(seat(DAISY.0).as_card("01002").at(STUDY).hand(DAISYS_HAND))
        .with_seating([ROLAND, DAISY, AGNES])
}

/// The board as `you` sees it after the `Hello` of `game` with `outcome`
/// live: Tal holding Roland and Agnes, and Sam holding Daisy.
pub(crate) async fn mount_table(
    you: AccountId,
    game: GameStateBuilder,
    outcome: EngineOutcome,
) -> BoardHandle {
    let mut client = ClientState {
        game_id: Some(GameId::new("g-table")),
        ..ClientState::default()
    };
    let frame = serde_json::to_string(&ServerMessage::Hello {
        build: protocol::build_id().to_string(),
        you,
        creator: YOU,
        members: members(),
        assigned: Assignment::from_iter([(ROLAND, YOU), (AGNES, YOU), (DAISY, SAM)]),
        prompt: LogPosition(0),
        state: Box::new(game.build()),
        outcome,
        events: Vec::new(),
    })
    .expect("a Hello serializes");
    store::receive(&mut client, &frame);
    common::mount_board(client, &cards::TEST_REGISTRY).await
}

/// The name on each tab, in order.
pub(crate) fn tab_names(handle: &BoardHandle) -> Vec<String> {
    lobby::texts(handle, ".inv-tab .tab-name")
}

/// The name of the investigator docked below the tabs.
pub(crate) fn docked(handle: &BoardHandle) -> String {
    let names = lobby::texts(handle, ".dock .investigator .inv-name");
    assert_eq!(names.len(), 1, "one investigator is docked: {names:?}");
    names[0].clone()
}

/// Click the tab named `name`, and let the board react.
pub(crate) async fn open_tab(handle: &BoardHandle, name: &str) {
    click_named(handle, ".inv-tab", ".tab-name", name);
    task::tick().await;
}

/// Click the one `sel` under `handle`'s root whose `name_sel` reads `name`.
fn click_named(handle: &BoardHandle, sel: &str, name_sel: &str, name: &str) {
    let all = handle.root.query_selector_all(sel).expect("query");
    let el = (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .find(|el| {
            el.query_selector(name_sel)
                .expect("query")
                .and_then(|n| n.text_content())
                .is_some_and(|text| text == name)
        })
        .unwrap_or_else(|| panic!("a {sel} named {name}"));
    el.dyn_into::<HtmlElement>()
        .expect("an HTML element")
        .click();
}

/// The printed name of `code`.
fn card_name(code: &str) -> String {
    (cards::TEST_REGISTRY.metadata_for)(&CardCode::new(code))
        .expect("a corpus card")
        .name
        .clone()
}

#[wasm_bindgen_test]
async fn the_tabs_render_in_seating_order_and_tint_the_ones_you_hold() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;

    assert_eq!(
        tab_names(&handle),
        vec!["Roland Banks", "Daisy Walker", "Agnes Baker"]
    );
    assert_eq!(
        lobby::texts(&handle, ".inv-tab.held .tab-name"),
        vec!["Roland Banks", "Agnes Baker"]
    );
    assert_eq!(
        lobby::texts(&handle, ".inv-tab .tab-lead"),
        vec!["★"],
        "Roland leads"
    );
}

#[wasm_bindgen_test]
async fn a_tab_click_switches_the_dock() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;
    assert_eq!(docked(&handle), "Roland Banks", "Tal's first in seating");

    open_tab(&handle, "Daisy Walker").await;
    assert_eq!(docked(&handle), "Daisy Walker");
    assert_eq!(
        lobby::texts(&handle, ".inv-tab.open .tab-name"),
        vec!["Daisy Walker"]
    );
}

#[wasm_bindgen_test]
async fn a_map_token_click_opens_their_tab() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;

    click_named(&handle, ".inv-token", ".token-name", "Agnes Baker");
    task::tick().await;
    assert_eq!(docked(&handle), "Agnes Baker");
}

#[wasm_bindgen_test]
async fn another_accounts_hand_is_a_count_with_no_cards_in_the_dom() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;

    open_tab(&handle, "Agnes Baker").await;
    assert_eq!(
        lobby::texts(&handle, ".dock .hand .hand-slot").len(),
        AGNESS_HAND.len(),
        "Tal's own hand shows its cards"
    );

    open_tab(&handle, "Daisy Walker").await;
    assert_eq!(lobby::texts(&handle, ".dock .hand-count"), vec!["2"]);
    assert!(lobby::texts(&handle, ".dock .hand-slot").is_empty());
    let page = handle.root.inner_html();
    for code in DAISYS_HAND {
        assert!(!page.contains(code), "{code} reached the DOM");
        let name = card_name(code);
        assert!(!page.contains(&name), "{name} reached the DOM");
    }
}

#[wasm_bindgen_test]
async fn a_viewer_sees_every_hand_as_a_count() {
    let handle = mount_table(ALEX, table(), EngineOutcome::Done).await;

    for (name, count) in [("Roland Banks", 2), ("Daisy Walker", 2), ("Agnes Baker", 4)] {
        open_tab(&handle, name).await;
        assert_eq!(docked(&handle), name);
        assert_eq!(
            lobby::texts(&handle, ".dock .hand-count"),
            vec![count.to_string()]
        );
        assert!(lobby::texts(&handle, ".dock .hand-slot").is_empty());
    }
}
