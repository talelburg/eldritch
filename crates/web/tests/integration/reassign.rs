//! Reassignment on the board: the ⋯ menu offers each account the
//! reassignments its role allows, and an `Assigned` moves the prompt to the
//! new holder.
//!
//! Each game is Tal's, with Sam and Jo as members: Sam holds Wendy Adams,
//! whose mulligan the game awaits, and Tal holds Roland Banks. Tal is [`YOU`].
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support::{self, seat};
use leptos::prelude::*;
use leptos::task;
use protocol::lobby::Member;
use protocol::{
    AccountId, Assignment, ClientMessage, GameId, LogPosition, Refusal, ServerMessage,
};
use wasm_bindgen_test::*;
use web::store::{self, ClientState};

use crate::common::{self, BoardHandle, YOU};
use crate::lobby::{click, sent, texts, SAM};

/// Jo, a member holding no investigator.
const JO: AccountId = AccountId(3);
/// Wendy Adams, Sam's, whose mulligan the game awaits.
const WENDY: InvestigatorId = InvestigatorId(1);
/// Roland Banks, Tal's.
const ROLAND: InvestigatorId = InvestigatorId(2);

/// Tal, Sam and Jo, in the order they joined.
fn members() -> Vec<Member> {
    [(YOU, "Tal"), (SAM, "Sam"), (JO, "Jo")]
        .into_iter()
        .map(|(account, name)| Member {
            account,
            display_name: name.to_string(),
        })
        .collect()
}

/// Sam holding Wendy and Tal holding Roland.
fn sam_wendy_tal_roland() -> Assignment {
    Assignment::from_iter([(WENDY, SAM), (ROLAND, YOU)])
}

/// The id of Tal's game.
pub(crate) const GAME_ID: &str = "g-tals";

/// The board as `you` sees it after the `Hello` of Tal's game at Wendy's
/// mulligan, or with `outcome` in its place when it is `Some`.
pub(crate) async fn mount_as(you: AccountId, outcome: Option<EngineOutcome>) -> BoardHandle {
    let state = GameStateBuilder::new()
        .with_registries(cards::TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01005"))
        .with_seat(seat(2).as_card("01001"))
        .with_seating([WENDY, ROLAND])
        .with_mulligan_remaining([WENDY, ROLAND])
        .build();
    // Settle through the engine, so the prompt is the one it actually builds.
    let seeded = test_support::TestSession::new(state, cards::TEST_REGISTRIES);
    let outcome =
        outcome.unwrap_or_else(|| test_support::awaiting_request(seeded.prompt().clone()));
    let mut client = ClientState {
        game_id: Some(GameId::new(GAME_ID)),
        ..ClientState::default()
    };
    let frame = serde_json::to_string(&ServerMessage::Hello {
        build: protocol::build_id().to_string(),
        you,
        creator: YOU,
        members: members(),
        assigned: sam_wendy_tal_roland(),
        prompt: LogPosition(0),
        state: Box::new(seeded.state().clone()),
        outcome,
        events: Vec::new(),
    })
    .expect("a Hello serializes");
    store::receive(&mut client, &frame);
    common::mount_board(client, &cards::TEST_REGISTRY).await
}

/// Open the board's ⋯ menu.
pub(crate) async fn open_menu(handle: &BoardHandle) {
    click(handle, ".board-menu .more-menu-toggle");
    task::tick().await;
}

/// Each heading of the open ⋯ menu, with the entries under it.
fn menu(handle: &BoardHandle) -> Vec<(String, Vec<String>)> {
    let groups = handle
        .root
        .query_selector_all(".board-menu .menu-group")
        .expect("query");
    (0..groups.length())
        .filter_map(|i| groups.item(i))
        .filter_map(|node| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(node).ok())
        .map(|group| {
            let heading = group
                .query_selector(".menu-heading")
                .expect("query")
                .and_then(|heading| heading.text_content())
                .unwrap_or_default();
            let items = group.query_selector_all(".menu-item").expect("query");
            let items = (0..items.length())
                .filter_map(|i| items.item(i))
                .map(|item| item.text_content().unwrap_or_default())
                .collect();
            (heading, items)
        })
        .collect()
}

/// Feed `handle`'s store `message`, as the transport would.
async fn arrives(handle: &BoardHandle, message: ServerMessage) {
    let frame = serde_json::to_string(&message).expect("a ServerMessage serializes");
    handle.store.update(|s| {
        store::receive(s, &frame);
    });
    task::tick().await;
}

/// The `Assigned` Sam's reassignment of Wendy to Tal broadcasts.
fn sam_gave_wendy_to_tal() -> ServerMessage {
    ServerMessage::Assigned {
        by: SAM,
        investigator: WENDY,
        assigned: Assignment::all_to(YOU, [WENDY, ROLAND]),
    }
}

/// `(heading, entries)` of one Reassign submenu, for comparing with [`menu`].
fn reassign(investigator: &str, to: &[&str]) -> (String, Vec<String>) {
    (
        format!("Reassign {investigator}"),
        to.iter().map(ToString::to_string).collect(),
    )
}

#[wasm_bindgen_test]
async fn the_creator_may_reassign_every_investigator_to_any_member_but_its_holder() {
    let handle = mount_as(YOU, None).await;

    open_menu(&handle).await;

    assert_eq!(
        menu(&handle),
        [
            reassign("Wendy Adams", &["Tal", "Jo"]),
            reassign("Roland Banks", &["Sam", "Jo"]),
        ]
    );
}

#[wasm_bindgen_test]
async fn a_holder_may_reassign_only_their_own_investigator() {
    let handle = mount_as(SAM, None).await;

    open_menu(&handle).await;

    assert_eq!(menu(&handle), [reassign("Wendy Adams", &["Tal", "Jo"])]);
}

#[wasm_bindgen_test]
async fn a_viewer_is_offered_no_reassignment() {
    let handle = mount_as(JO, None).await;

    assert!(
        texts(&handle, ".board-menu .more-menu-toggle").is_empty(),
        "a viewer's ⋯ menu has nothing in it"
    );
}

#[wasm_bindgen_test]
async fn a_finished_game_offers_no_reassignment() {
    let handle = mount_as(YOU, Some(EngineOutcome::Done)).await;

    open_menu(&handle).await;

    assert!(menu(&handle).is_empty(), "no Reassign submenu");
}

#[wasm_bindgen_test]
async fn picking_a_member_sends_the_reassignment() {
    let mut handle = mount_as(YOU, None).await;
    open_menu(&handle).await;

    // Jo, under Wendy Adams.
    click(&handle, ".board-menu .menu-group .menu-item:last-child");
    task::tick().await;

    assert!(matches!(
        sent(&mut handle),
        ClientMessage::Reassign {
            investigator: WENDY,
            to: JO
        }
    ));
}

#[wasm_bindgen_test]
async fn assigned_says_who_gave_whom_to_whom_and_moves_the_prompt_to_the_new_holder() {
    let handle = mount_as(YOU, None).await;
    assert!(
        texts(&handle, ".prompt-banner").is_empty(),
        "Wendy's mulligan is Sam's to answer"
    );

    arrives(&handle, sam_gave_wendy_to_tal()).await;

    assert_eq!(texts(&handle, ".toast"), ["Sam gave Wendy Adams to Tal"]);
    assert_eq!(
        texts(&handle, ".prompt-banner .confirm").len(),
        1,
        "Tal answers Wendy's mulligan"
    );
}

#[wasm_bindgen_test]
async fn a_not_yours_after_the_reassignment_that_caused_it_stays_quiet() {
    let mut handle = mount_as(SAM, None).await;
    // Sam keeps Wendy's opening hand as Wendy goes to Tal.
    click(&handle, ".prompt-banner .confirm");
    task::tick().await;
    assert!(matches!(sent(&mut handle), ClientMessage::Submit { .. }));

    arrives(&handle, sam_gave_wendy_to_tal()).await;
    arrives(&handle, ServerMessage::Rejected(Refusal::NotYours)).await;

    assert_eq!(
        handle.store.with_untracked(|s| s.last_rejection.clone()),
        None
    );
}

#[wasm_bindgen_test]
async fn a_not_yours_for_an_investigator_still_held_is_still_a_client_bug() {
    let mut handle = mount_as(SAM, None).await;
    click(&handle, ".prompt-banner .confirm");
    task::tick().await;
    let _ = sent(&mut handle);

    arrives(&handle, ServerMessage::Rejected(Refusal::NotYours)).await;

    assert!(
        handle.store.with_untracked(|s| s.last_rejection.is_some()),
        "Sam still holds Wendy, so the refusal is a bug"
    );
}
