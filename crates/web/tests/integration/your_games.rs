//! The "Your games" page at `/`: the listing `GET /games` returned, set in
//! the store, rendered in its groups, newest first, with New game in the
//! page's heading.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::scenario::{ResolutionId, ScenarioEnding, ScenarioId};
use game_core::state::{CardCode, InvestigatorId, Phase};
use leptos::task;
use protocol::live::{LiveSummary, WaitedOn};
use protocol::lobby::Member;
use protocol::{
    AccountId, EntryInvestigator, GameEntry, GameEntryState, GameId, ReplayCause, SignedIn,
    UnresumableCause, UnresumableSummary,
};
use wasm_bindgen::JsCast as _;
use web::socket_ending::{NextStep, SocketEnding};
use web::store::{ClientState, ConnStatus};
use web::url;
use web_sys::Element;

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
        creator: Some(creator),
        investigators: vec![EntryInvestigator {
            investigator: CardCode::new(DAISY),
            holder: Some(SAM),
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

/// [`mount_page`], signed in as `account`.
async fn mount_page_as(account: AccountId, listing: Vec<GameEntry>) -> BoardHandle {
    let client = ClientState {
        status: ConnStatus::Home,
        games: Some(listing),
        account: Some(SignedIn {
            account_id: account,
            display_name: "Tal".to_string(),
        }),
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
        holder: Some(TAL),
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
        holder: Some(TAL),
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
        finished: None,
    });
    entry
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn the_lead_investigator_alone_wears_the_star() {
    let handle = mount_page(Some(vec![thursday(
        "g-thursday",
        1_000,
        &[(2, ROLAND, TAL)],
    )]))
    .await;

    let seats = handle
        .root
        .query_selector_all(".game-row .row-investigator")
        .expect("query");
    let starred: Vec<(String, bool)> = (0..seats.length())
        .filter_map(|i| seats.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .map(|seat| {
            let star = seat.query_selector(".lead-star").expect("query");
            (seat.text_content().unwrap_or_default(), star.is_some())
        })
        .collect();
    assert_eq!(
        starred.len(),
        2,
        "both investigators are listed: {starred:?}"
    );
    assert!(
        starred[0].0.contains("Daisy Walker") && starred[0].1,
        "Daisy, who leads, wears the star: {starred:?}"
    );
    assert!(
        starred[1].0.contains("Roland Banks") && !starred[1].1,
        "Roland, who doesn't, does not: {starred:?}"
    );
}

/// Thursday's game, finished at `ending`, its last action at `at`.
fn finished(game_id: &str, at: i64, ending: ScenarioEnding) -> GameEntry {
    let mut entry = thursday(game_id, at, &[]);
    if let GameEntryState::Live(summary) = &mut entry.state {
        summary.finished = Some(ending);
    }
    entry
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn finished_games_are_listed_last_under_finished_with_the_ending_reached() {
    let handle = mount_page(Some(vec![
        finished("g-no-resolution", 4_000, ScenarioEnding::NoResolution),
        thursday("g-thursday", 1_000, &[(1, DAISY, SAM)]),
        finished(
            "g-resolved",
            5_000,
            ScenarioEnding::Resolution(ResolutionId::new(1)),
        ),
        thursday("g-tals-turn", 2_000, &[(2, ROLAND, TAL)]),
    ]))
    .await;

    assert_eq!(
        texts(&handle, ".games-group-title"),
        ["Needs you", "In progress", "Finished"]
    );
    assert_eq!(
        texts(&handle, ".games-group:last-of-type .row-status"),
        ["Resolution 1", "No resolution reached"],
        "each finished game says the ending it reached, newest first"
    );
    assert_eq!(
        texts(&handle, ".game-row .row-status").len(),
        4,
        "a finished game is listed under no other group"
    );
    assert_eq!(texts(&handle, ".row-pill"), ["Daisy Walker needs you"]);
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

/// Thursday's game, last moving at `at`, which this build can't replay for
/// `reason`, with 340 answers logged, created on build `"older-build"` and
/// listed by a server running `"newer-build"`.
pub(crate) fn stale(game_id: &str, at: i64, reason: UnresumableCause) -> GameEntry {
    let mut entry = thursday(game_id, at, &[]);
    entry.state = GameEntryState::Unresumable(UnresumableSummary {
        reason,
        logged_answers: 340,
        created_build: "older-build".to_string(),
        server_build: "newer-build".to_string(),
    });
    entry
}

/// The cause of a game stopped by its answer at zero-based `seq` failing
/// `cause`.
fn replay(seq: u32, cause: ReplayCause) -> UnresumableCause {
    UnresumableCause::Replay { seq, cause }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn an_unresumable_game_is_listed_last_with_where_it_stopped_and_why() {
    let label = "Investigate: Hallway";
    let not_found = ReplayCause::OptionNotFound {
        label: label.to_string(),
        matches: 0,
    };
    let handle = mount_page(Some(vec![
        stale("g-stale", 3_000, replay(214, not_found)),
        thursday("g-tals-turn", 2_000, &[(2, ROLAND, TAL)]),
        lobby("g-lobby", TAL, "Tal", 1_000),
    ]))
    .await;

    assert_eq!(
        texts(&handle, ".games-group-title"),
        ["Lobby", "In progress", "Can't resume on this build"]
    );
    let row = handle
        .root
        .query_selector(r#".game-row[href="/games/g-stale"]"#)
        .expect("query")
        .expect("the unresumable game is listed, at its link");
    let text = row.text_content().unwrap_or_default();
    for shown in [
        r#"Stopped at action 215 of 340: no option labelled "Investigate: Hallway""#,
        "The Gathering",
        "Created by Tal",
        "Daisy Walker",
        "Roland Banks",
    ] {
        assert!(text.contains(shown), "the row shows {shown}: {text}");
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn each_reason_a_game_cannot_resume_reads_in_plain_words() {
    let option = |matches| ReplayCause::OptionNotFound {
        label: "Investigate: Hallway".to_string(),
        matches,
    };
    let handle = mount_page(Some(vec![
        stale("g-1", 9_000, replay(0, option(2))),
        stale("g-2", 8_000, replay(1, ReplayCause::PromptChanged)),
        stale("g-3", 7_000, replay(2, ReplayCause::Rejected)),
        stale("g-4", 6_000, replay(3, ReplayCause::Draws)),
        stale("g-5", 5_000, UnresumableCause::Fingerprint),
        stale(
            "g-6",
            4_000,
            UnresumableCause::Setup("unknown scenario: the-gathering".to_string()),
        ),
        stale("g-7", 3_000, UnresumableCause::Version),
        stale("g-8", 2_000, UnresumableCause::NoAssignment),
    ]))
    .await;

    assert_eq!(
        texts(&handle, ".game-row .row-status"),
        [
            r#"Stopped at action 1 of 340: more than one option labelled "Investigate: Hallway""#,
            "Stopped at action 2 of 340: the game now asks something else here",
            "Stopped at action 3 of 340: the engine now refuses that answer",
            "Stopped at action 4 of 340: the shuffle now comes out differently",
            "Setup now deals differently",
            "Setup no longer runs: unknown scenario: the-gathering",
            "Stored by an older version of the server",
            "Stored by an older version of the server",
        ]
    );
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn home_after_a_no_longer_a_member_close_says_why_in_a_toast() {
    // The address the close sends the client to, read back as the transport
    // reads it on arriving at the home page.
    let NextStep::GoHome(ended) = SocketEnding::NoLongerMember.next_step() else {
        panic!("the close goes home");
    };
    let address = url::home_path(ended);
    let query = &address[address.find('?').expect("the address names the ending")..];
    let client = ClientState {
        status: ConnStatus::Home,
        games: Some(vec![]),
        toast: url::ended_from_query(query).map(|ended| ended.toast().to_string()),
        ..ClientState::default()
    };
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;

    assert_eq!(
        texts(&handle, ".toast"),
        ["You're no longer a member of this game"]
    );
    assert_eq!(texts(&handle, ".home .games-heading").len(), 1, "on Home");
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn an_unresumable_game_with_no_creator_or_holders_still_lists_its_investigators() {
    let mut entry = stale("g-old", 1_000, UnresumableCause::NoAssignment);
    entry.creator = None;
    for seat in &mut entry.investigators {
        seat.holder = None;
    }
    let handle = mount_page(Some(vec![entry])).await;

    assert!(
        texts(&handle, ".row-holder").is_empty(),
        "no holder is shown"
    );
    let text = texts(&handle, ".game-row").concat();
    assert!(text.contains("Daisy Walker"), "{text}");
    assert!(!text.contains("Created by"), "no creator is named: {text}");
}

/// One game of every kind Tal created, and a lobby Alex did.
fn tals_games_and_alexs_lobby() -> Vec<GameEntry> {
    vec![
        lobby("g-lobby", TAL, "Tal", 5_000),
        lobby("g-alex", AccountId(3), "Alex", 4_000),
        thursday("g-live", 3_000, &[(1, DAISY, SAM)]),
        finished("g-done", 2_000, ScenarioEnding::NoResolution),
        stale("g-stale", 1_000, UnresumableCause::Version),
    ]
}

/// The games whose row has a ⋯ menu, in document order.
fn rows_with_a_menu(handle: &BoardHandle) -> Vec<String> {
    let nodes = handle
        .root
        .query_selector_all(".game-row-item")
        .expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .filter(|item| {
            item.query_selector(".more-menu-toggle")
                .expect("query")
                .is_some()
        })
        .filter_map(|item| item.get_attribute("data-game"))
        .collect()
}

/// Click the one `sel` under `handle`'s root, and let the page react.
async fn click(handle: &BoardHandle, sel: &str) {
    handle
        .root
        .query_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("the page renders {sel}"))
        .dyn_into::<web_sys::HtmlElement>()
        .expect("an HTML element")
        .click();
    task::tick().await;
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn delete_is_on_every_kind_of_row_the_account_created_and_no_other() {
    let handle = mount_page_as(TAL, tals_games_and_alexs_lobby()).await;

    let mut with_a_menu = rows_with_a_menu(&handle);
    with_a_menu.sort();
    assert_eq!(with_a_menu, ["g-done", "g-live", "g-lobby", "g-stale"]);
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_member_who_created_none_of_the_games_has_no_delete() {
    let handle = mount_page_as(SAM, tals_games_and_alexs_lobby()).await;

    assert!(rows_with_a_menu(&handle).is_empty());
}

/// The only way to delete an unresumable game, which never opens a room.
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_rows_delete_asks_first_then_deletes_that_game() {
    let mut handle = mount_page_as(TAL, tals_games_and_alexs_lobby()).await;
    let row = r#".game-row-item[data-game="g-stale"]"#;

    click(&handle, &format!("{row} .more-menu-toggle")).await;
    assert_eq!(texts(&handle, &format!("{row} .menu-item")), ["Delete"]);
    click(&handle, &format!("{row} .menu-item")).await;

    assert_eq!(
        texts(&handle, ".confirm-dialog .confirm-question"),
        ["Delete for everyone? This can't be undone."]
    );
    assert!(handle.deleted.try_recv().is_err(), "nothing is deleted yet");

    click(&handle, ".confirm-dialog .confirm-delete").await;

    assert_eq!(handle.deleted.try_recv().ok(), Some(GameId::new("g-stale")));
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn home_after_a_deleted_close_says_why_in_a_toast() {
    let NextStep::GoHome(ended) = SocketEnding::Deleted.next_step() else {
        panic!("the close goes home");
    };
    let address = url::home_path(ended);
    let query = &address[address.find('?').expect("the address names the ending")..];
    let client = ClientState {
        status: ConnStatus::Home,
        games: Some(vec![]),
        toast: url::ended_from_query(query).map(|ended| ended.toast().to_string()),
        ..ClientState::default()
    };
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;

    assert_eq!(texts(&handle, ".toast"), ["This game was deleted"]);
    assert_eq!(texts(&handle, ".home .games-heading").len(), 1, "on Home");
}
