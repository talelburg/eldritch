//! The tabs over the dock: one tab per investigator, in seating order, and the
//! open tab's investigator docked below them.
//!
//! Each game is [`mount_table`]'s: Tal, [`YOU`], holds Roland Banks and Agnes
//! Baker, Sam holds Daisy Walker, and Alex is a viewer.
#![cfg(target_arch = "wasm32")]

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, EngineOutcome, InputRequest, OptionId, Responder};
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, location, seat, TestSession};
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::Member;
use protocol::logged_answer::LoggedResponse;
use protocol::{AccountId, Applied, Assignment, ClientMessage, GameId, LogPosition, ServerMessage};
use wasm_bindgen_test::*;
use web::store::{self, ClientState};
use web_sys::Element;

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
    mount_table_at(you, game.build(), outcome).await
}

/// [`mount_table`] of a game already built, such as the state a test session
/// rests at.
pub(crate) async fn mount_table_at(
    you: AccountId,
    game: GameState,
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
        state: Box::new(game),
        outcome,
        events: Vec::new(),
    })
    .expect("a Hello serializes");
    store::receive(&mut client, &frame);
    common::mount_board(client, &cards::TEST_REGISTRY).await
}

/// The board as `you` sees the prompt `session` rests at.
pub(crate) async fn mount_as(you: AccountId, session: &TestSession) -> BoardHandle {
    let outcome = test_support::awaiting_request(session.prompt().clone());
    mount_table_at(you, session.state().clone(), outcome).await
}

/// The name on each tab, in order.
pub(crate) fn tab_names(handle: &BoardHandle) -> Vec<String> {
    lobby::texts(handle, ".inv-tab .tab-name")
}

/// The name of the investigator docked below the tabs, read off their
/// investigator card.
pub(crate) fn docked(handle: &BoardHandle) -> String {
    let names = lobby::texts(handle, ".dock .investigator-card .tile-name");
    assert_eq!(names.len(), 1, "one investigator is docked: {names:?}");
    names[0].clone()
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

/// The tab and the investigator card already name the docked investigator, so
/// the dock repeats the name in no heading of its own.
#[wasm_bindgen_test]
async fn the_dock_names_its_investigator_only_on_the_tab_and_card() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;
    let dock = handle
        .root
        .query_selector(".dock .investigator")
        .expect("query")
        .expect("a docked investigator");
    assert!(
        dock.query_selector("h1, h2, h3, h4, .inv-name")
            .expect("query")
            .is_none(),
        "the dock has no heading: {}",
        dock.inner_html()
    );
    assert_eq!(docked(&handle), "Roland Banks");
}

/// The hand row of the docked investigator.
fn hand_row(handle: &BoardHandle) -> Element {
    handle
        .root
        .query_selector(".dock .hand .card-row")
        .expect("query")
        .expect("the docked hand's row")
}

/// Each tab opens on its own hand row, scrolled to the start, and an update to
/// the open tab leaves its row where it was.
#[wasm_bindgen_test]
async fn each_tab_opens_its_own_hand_row_scrolled_to_the_start() {
    common::inject_style();
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;
    // A row far narrower than its cards, scoped to this board alone, so every
    // hand overflows it whatever the page's width.
    handle
        .root
        .class_list()
        .add_1("narrow-hand")
        .expect("class");
    let style = document().create_element("style").expect("create style");
    style.set_text_content(Some(".narrow-hand .dock .hand .card-row { width: 40px; }"));
    handle.root.append_child(&style).expect("append style");

    hand_row(&handle).set_scroll_left(30);
    assert_eq!(
        hand_row(&handle).scroll_left(),
        30,
        "Roland's hand overflows"
    );
    handle.store.update(|s| {
        s.game.as_mut().expect("a game").round += 1;
    });
    task::tick().await;
    assert_eq!(hand_row(&handle).scroll_left(), 30, "an update keeps it");

    common::click_tab(&handle.root, "Agnes Baker").await;
    assert_eq!(
        hand_row(&handle).scroll_left(),
        0,
        "Agnes's row starts at 0"
    );
    common::click_tab(&handle.root, "Roland Banks").await;
    assert_eq!(hand_row(&handle).scroll_left(), 0, "so does Roland's again");
}

#[wasm_bindgen_test]
async fn a_tab_click_switches_the_dock() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;
    assert_eq!(docked(&handle), "Roland Banks", "Tal's first in seating");

    common::click_tab(&handle.root, "Daisy Walker").await;
    assert_eq!(docked(&handle), "Daisy Walker");
    assert_eq!(
        lobby::texts(&handle, ".inv-tab.open .tab-name"),
        vec!["Daisy Walker"]
    );
}

#[wasm_bindgen_test]
async fn a_map_token_click_opens_their_tab() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;

    common::click_named(&handle.root, ".inv-token", ".token-name", "Agnes Baker");
    task::tick().await;
    assert_eq!(docked(&handle), "Agnes Baker");
}

#[wasm_bindgen_test]
async fn another_accounts_hand_is_a_count_with_no_cards_in_the_dom() {
    let handle = mount_table(YOU, table(), EngineOutcome::Done).await;

    common::click_tab(&handle.root, "Agnes Baker").await;
    assert_eq!(
        lobby::texts(&handle, ".dock .hand .hand-slot").len(),
        AGNESS_HAND.len(),
        "Tal's own hand shows its cards"
    );

    common::click_tab(&handle.root, "Daisy Walker").await;
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
        common::click_tab(&handle.root, name).await;
        assert_eq!(docked(&handle), name);
        assert_eq!(
            lobby::texts(&handle, ".dock .hand-count"),
            vec![count.to_string()]
        );
        assert!(lobby::texts(&handle, ".dock .hand-slot").is_empty());
    }
}

/// Agnes's prompt: a skippable choice of what to discard, which Tal answers.
fn agness_discard() -> EngineOutcome {
    test_support::awaiting_request(
        InputRequest::pick_single("Choose a card to discard", vec![])
            .skippable()
            .answered_by(Responder::Investigator(AGNES)),
    )
}

/// The `Applied` that answers the prompt at log position 0 and leaves
/// `outcome` live, with the table's game as it was.
fn next_prompt(outcome: EngineOutcome) -> ServerMessage {
    ServerMessage::Applied(Applied {
        answered: LogPosition(0),
        by: ROLAND,
        response: LoggedResponse::Confirm,
        prompt: LogPosition(1),
        state: Box::new(table().build()),
        events: Vec::new(),
        outcome,
    })
}

#[wasm_bindgen_test]
async fn a_new_prompt_for_agnes_opens_her_tab_with_her_strip_in_the_dock() {
    let mut handle = mount_table(YOU, table(), EngineOutcome::Done).await;
    assert_eq!(docked(&handle), "Roland Banks");

    common::arrives(&handle, next_prompt(agness_discard())).await;
    task::tick().await;

    assert_eq!(docked(&handle), "Agnes Baker");
    assert_eq!(
        lobby::texts(&handle, ".dock .investigator .prompt-strip .strip-name"),
        vec!["Agnes Baker"]
    );
    assert_eq!(lobby::texts(&handle, ".prompt-strip").len(), 1);
    lobby::click(&handle, ".dock .prompt-strip .pass");
    task::tick().await;
    match lobby::sent(&mut handle) {
        ClientMessage::Submit {
            action:
                PlayerAction::ResolveInput {
                    investigator,
                    response,
                },
            ..
        } => assert_eq!((investigator, response), (AGNES, InputResponse::Skip)),
        other => panic!("expected a ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn agness_tab_needs_tal_while_he_reads_rolands() {
    let handle = mount_table(YOU, table(), agness_discard()).await;
    assert_eq!(docked(&handle), "Agnes Baker");
    assert!(lobby::texts(&handle, ".inv-tab.needs-you").is_empty());

    common::click_tab(&handle.root, "Roland Banks").await;
    assert_eq!(docked(&handle), "Roland Banks");
    assert!(lobby::texts(&handle, ".prompt-strip").is_empty());
    assert_eq!(
        lobby::texts(&handle, ".inv-tab.needs-you .tab-name"),
        vec!["Agnes Baker"]
    );
}

#[wasm_bindgen_test]
async fn the_waiting_strip_hides_while_tal_reads_daisys_tab() {
    let daisys_horror = test_support::awaiting_request(
        InputRequest::confirm("Assign the horror").answered_by(Responder::Investigator(DAISY)),
    );
    let handle = mount_table(YOU, table(), daisys_horror).await;
    assert_eq!(lobby::texts(&handle, ".waiting-strip").len(), 1);

    common::click_tab(&handle.root, "Daisy Walker").await;
    assert!(lobby::texts(&handle, ".waiting-strip").is_empty());
}

/// The lead picks who takes the next turn, and each choice is anchored to that
/// investigator's card. Only the open tab's card is on screen, so every tab
/// whose investigator card carries an option glows as the card does, and the
/// lead sees the choices without opening each tab.
#[wasm_bindgen_test]
async fn a_tab_glows_while_its_investigator_card_carries_an_option() {
    let game = table().build();
    let on_card = |id: u32, investigator: InvestigatorId| {
        ChoiceOption::new(OptionId(id), "Take the next turn")
            .at(game.investigators[&investigator].card_anchor())
    };
    let next_turn = test_support::awaiting_request(
        InputRequest::pick_single(
            "Choose who takes the next turn",
            vec![on_card(0, ROLAND), on_card(1, AGNES)],
        )
        .answered_by(Responder::Lead(ROLAND)),
    );
    let handle = mount_table(YOU, table(), next_turn).await;

    assert_eq!(
        lobby::texts(&handle, ".inv-tab.actionable .tab-name"),
        vec!["Roland Banks", "Agnes Baker"]
    );
}
