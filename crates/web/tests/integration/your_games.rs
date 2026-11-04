//! The "Your games" page at `/`: the listing `GET /games` returned, set in
//! the store, rendered in its groups, newest first, with New game in the
//! page's heading.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::scenario::ScenarioId;
use game_core::state::{CardCode, InvestigatorId, Phase};
use leptos::task;
use protocol::live::{LiveSummary, WaitedOn};
use protocol::lobby::Member;
use protocol::{AccountId, EntryInvestigator, GameEntry, GameEntryState, GameId};
use web::store::{ClientState, ConnStatus};

use crate::common::{self, BoardHandle};

/// Tal, who creates the games.
const TAL: AccountId = AccountId(1);
/// Sam, signed in and looking at the page.
const SAM: AccountId = AccountId(2);
/// Daisy Walker 01002.
const DAISY: &str = "01002";
/// Roland Banks 01001.
const ROLAND: &str = "01001";

/// A lobby of The Gathering created by `creator`, named `creator_name`, at
/// `created_at`, in which Sam has taken Daisy, who leads.
fn lobby(game_id: &str, creator: AccountId, creator_name: &str, created_at: i64) -> GameEntry {
    GameEntry {
        game_id: GameId::new(game_id),
        scenario_id: ScenarioId::new("the-gathering"),
        creator,
        investigators: vec![EntryInvestigator {
            investigator: CardCode::new(DAISY),
            holder: SAM,
            lead: true,
        }],
        names: vec![
            Member {
                account: creator,
                display_name: creator_name.to_string(),
            },
            Member {
                account: SAM,
                display_name: "Sam".to_string(),
            },
        ],
        last_activity: created_at,
        state: GameEntryState::Lobby,
    }
}

/// Mount the page at `Home` with `listing` in the store.
async fn mount_page(listing: Option<Vec<GameEntry>>) -> BoardHandle {
    let client = ClientState {
        status: ConnStatus::Home,
        games: listing,
        ..ClientState::default()
    };
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;
    handle
}

/// The text of every `sel` under `handle`'s root, in document order.
fn texts(handle: &BoardHandle, sel: &str) -> Vec<String> {
    let nodes = handle.root.query_selector_all(sel).expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .map(|node| node.text_content().unwrap_or_default())
        .collect()
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn lobbies_are_listed_under_lobby_newest_first_each_waiting_for_its_creator() {
    let mut tals = lobby("g-tal", TAL, "Tal", 1_000);
    tals.investigators.push(EntryInvestigator {
        investigator: CardCode::new(ROLAND),
        holder: TAL,
        lead: false,
    });
    let handle = mount_page(Some(vec![
        tals,
        lobby("g-newer", AccountId(3), "Alex", 2_000),
    ]))
    .await;

    assert_eq!(texts(&handle, ".games-group-title"), ["Lobby"]);
    assert_eq!(
        texts(&handle, ".games-group .game-row .row-status"),
        ["Waiting for Alex to start", "Waiting for Tal to start"],
        "newest first"
    );
    let row = handle
        .root
        .query_selector(r#".game-row[href="/games/g-tal"]"#)
        .expect("query")
        .expect("Tal's row links to the game");
    let text = row.text_content().unwrap_or_default();
    for shown in [
        "The Gathering",
        "Daisy Walker",
        "Sam",
        "Roland Banks",
        "Tal",
    ] {
        assert!(text.contains(shown), "the row shows {shown}: {text}");
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn new_game_sits_in_the_heading_and_an_empty_listing_shows_no_group() {
    let handle = mount_page(Some(vec![])).await;

    assert_eq!(
        texts(&handle, ".home .games-heading .home-new-game"),
        ["New game"]
    );
    assert!(texts(&handle, ".games-group").is_empty(), "no empty group");
}

/// Thursday's game, created by Tal, with Sam's Daisy leading and Tal's
/// Roland, in round 3's Investigation phase, last moving at `at`, waiting on
/// each of `waiting_on` as Sam sees it.
fn thursday(game_id: &str, at: i64, waiting_on: &[(u32, &str, AccountId)]) -> GameEntry {
    let mut entry = lobby(game_id, TAL, "Tal", at);
    entry.investigators.push(EntryInvestigator {
        investigator: CardCode::new(ROLAND),
        holder: TAL,
        lead: false,
    });
    entry.state = GameEntryState::Live(LiveSummary {
        round: 3,
        phase: Phase::Investigation,
        waiting_on: waiting_on
            .iter()
            .map(|&(id, card, holder)| WaitedOn {
                investigator: InvestigatorId(id),
                card: CardCode::new(card),
                holder: Some(holder),
                yours: holder == SAM,
            })
            .collect(),
    });
    entry
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_game_waiting_on_your_investigator_is_listed_first_under_needs_you_naming_it() {
    let handle = mount_page(Some(vec![
        lobby("g-lobby", TAL, "Tal", 3_000),
        thursday("g-tals-turn", 2_000, &[(2, ROLAND, TAL)]),
        thursday("g-thursday", 1_000, &[(1, DAISY, SAM)]),
    ]))
    .await;

    assert_eq!(
        texts(&handle, ".games-group-title"),
        ["Needs you", "Lobby", "In progress"]
    );
    let needs_you = handle
        .root
        .query_selector(r#".game-row[href="/games/g-thursday"]"#)
        .expect("query")
        .expect("Thursday's game is listed");
    assert_eq!(texts(&handle, ".row-pill"), ["Daisy Walker needs you"]);
    let text = needs_you.text_content().unwrap_or_default();
    assert!(
        text.contains("Round 3 · Investigation · waiting for Sam"),
        "{text}"
    );
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_game_waiting_on_another_account_is_in_progress_with_its_round_phase_and_who() {
    let handle = mount_page(Some(vec![thursday(
        "g-tals-turn",
        2_000,
        &[(2, ROLAND, TAL)],
    )]))
    .await;

    assert_eq!(texts(&handle, ".games-group-title"), ["In progress"]);
    assert_eq!(
        texts(&handle, ".game-row .row-status"),
        ["Round 3 · Investigation · waiting for Tal"]
    );
    assert!(texts(&handle, ".row-pill").is_empty(), "nothing is Sam's");
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_window_waiting_on_two_accounts_names_both_and_needs_you() {
    let handle = mount_page(Some(vec![thursday(
        "g-window",
        2_000,
        &[(1, DAISY, SAM), (2, ROLAND, TAL)],
    )]))
    .await;

    assert_eq!(texts(&handle, ".games-group-title"), ["Needs you"]);
    assert_eq!(
        texts(&handle, ".game-row .row-status"),
        ["Round 3 · Investigation · waiting for Sam and Tal"]
    );
    assert_eq!(texts(&handle, ".row-pill"), ["Daisy Walker needs you"]);
}
