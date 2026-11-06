//! Start: the creator seats the picks, and every connected member moves to
//! the board, each account answering only for its own investigators.
//!
//! These lobbies are of The Gathering, served the production registries, so
//! Start seats real investigators with their default decks.

use cards::decks::WeaknessChoice;
use game_core::action::RosterEntry;
use game_core::card_registry::CardRegistry;
use game_core::engine::Registries;
use game_core::state::investigator::InvestigatorId;
use game_core::state::CardCode;
use protocol::lobby::{Direction, LobbyRefusal, Member};
use protocol::{AccountId, Assignment, ClientMessage, GameId, Refusal, ServerMessage};
use server::session::GameSession;
use sqlx::SqlitePool;

use super::{
    daisy_roland_agnes, keep_opening_hand, move_in_seating, recv_hello, recv_lobby, recv_refusal,
    set_lead, started_by_tal_and_sam, take, take_seen_by_both, tal_and_sam_at_the_gathering, AGNES,
    DAISY, JO, ROLAND, WENDY,
};
use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT};

/// The production card registry, but with an empty weakness pool, so the
/// engine refuses to seat any pick whose weakness is random.
const NO_WEAKNESS_POOL: CardRegistry = CardRegistry {
    weakness_pool: || &[],
    ..cards::REGISTRY
};

/// The production registries, but with [`NO_WEAKNESS_POOL`].
const NO_RANDOM_WEAKNESS: Registries = Registries {
    cards: &NO_WEAKNESS_POOL,
    ..server::REGISTRIES
};

/// The roster and lead index stored as the creation inputs of the game
/// `game_id`.
async fn stored_inputs(pool: &SqlitePool, game_id: &str) -> (Vec<RosterEntry>, i64) {
    let (roster, lead): (String, i64) =
        sqlx::query_as("SELECT roster, lead FROM games WHERE game_id = ?")
            .bind(game_id)
            .fetch_one(pool)
            .await
            .expect("Start stored the game's creation inputs");
    (
        serde_json::from_str(&roster).expect("the stored roster parses"),
        lead,
    )
}

#[tokio::test]
async fn only_the_creator_may_start() {
    let mut members = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    take_seen_by_both(&mut members.sam_ws, &mut members.tal_ws, DAISY).await;

    common::send(&mut members.sam_ws, &ClientMessage::Start).await;

    assert_eq!(
        recv_refusal(&mut members.sam_ws).await,
        LobbyRefusal::NotCreator
    );
}

/// Tal reorders the seating and moves the lead before Start, so the roster
/// order, the lead and the assignment are each ones a command rewrote.
#[tokio::test]
async fn start_moves_every_member_to_the_board_seated_as_the_lobby_was() {
    let mut members = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    daisy_roland_agnes(&mut members).await;
    // Daisy, Roland, Agnes becomes Roland, Daisy, Agnes, with Agnes leading.
    move_in_seating(&mut members.tal_ws, DAISY, Direction::Later).await;
    let _ = recv_lobby(&mut members.tal_ws).await;
    let _ = recv_lobby(&mut members.sam_ws).await;
    set_lead(&mut members.tal_ws, AGNES).await;
    let _ = recv_lobby(&mut members.tal_ws).await;
    let _ = recv_lobby(&mut members.sam_ws).await;

    common::send(&mut members.tal_ws, &ClientMessage::Start).await;

    let seated = Assignment::from_iter([
        (InvestigatorId(1), members.tal),
        (InvestigatorId(2), members.sam),
        (InvestigatorId(3), members.tal),
    ]);
    assert_eq!(
        recv_hello(&mut members.tal_ws).await,
        (members.tal, seated.clone())
    );
    assert_eq!(
        recv_hello(&mut members.sam_ws).await,
        (members.sam, seated.clone())
    );

    // What was stored, read back as a restart would.
    let stored = GameSession::load(
        members.pool.clone(),
        server::REGISTRIES,
        &GameId::new(&members.game_id),
    )
    .await
    .expect("the started game loads")
    .expect("Start stored the game under the lobby's id");
    let state = stored.live().state();
    let names: Vec<&str> = state
        .seating
        .iter()
        .map(|id| state.investigators[id].name.as_str())
        .collect();
    assert_eq!(names, ["Roland Banks", "Daisy Walker", "Agnes Baker"]);
    assert_eq!(state.lead, Some(InvestigatorId(3)), "Agnes leads");
    assert_eq!(stored.live().assignment(), &seated);
    let (roster, lead) = stored_inputs(&members.pool, &members.game_id).await;
    let investigators: Vec<&str> = roster
        .iter()
        .map(|entry| entry.investigator.as_str())
        .collect();
    assert_eq!(investigators, [ROLAND, DAISY, AGNES], "the stored roster");
    assert_eq!(lead, 2, "the stored lead indexes Agnes");

    // The roster is frozen.
    take(&mut members.sam_ws, WENDY).await;
    assert_eq!(
        recv_refusal(&mut members.sam_ws).await,
        LobbyRefusal::Started
    );
}

#[tokio::test]
async fn a_random_weakness_pick_is_stored_drawing_one_random_basic_weakness() {
    let mut members = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    take_seen_by_both(&mut members.tal_ws, &mut members.sam_ws, ROLAND).await;
    common::send(
        &mut members.tal_ws,
        &ClientMessage::SetWeakness {
            investigator: CardCode::new(ROLAND),
            choice: WeaknessChoice::Random,
        },
    )
    .await;
    let _ = recv_lobby(&mut members.tal_ws).await;

    common::send(&mut members.tal_ws, &ClientMessage::Start).await;
    let _ = recv_hello(&mut members.tal_ws).await;

    let (roster, _) = stored_inputs(&members.pool, &members.game_id).await;
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0].investigator.as_str(), ROLAND);
    assert_eq!(roster[0].random_basic_weaknesses, 1);
    assert!(
        !roster[0].deck.contains(&CardCode::new("01102")),
        "his default deck's fixed weakness, Silver Twilight Acolyte 01102, is left out"
    );
}

#[tokio::test]
async fn after_start_sam_answers_for_daisy_and_is_refused_for_roland() {
    let mut members = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    daisy_roland_agnes(&mut members).await;
    common::send(&mut members.tal_ws, &ClientMessage::Start).await;
    let _ = recv_hello(&mut members.tal_ws).await;
    let _ = recv_hello(&mut members.sam_ws).await;

    // Roland is investigator 2, Tal's.
    common::send(&mut members.sam_ws, &keep_opening_hand(2)).await;
    match common::recv_within(&mut members.sam_ws).await {
        ServerMessage::Rejected(Refusal::NotYours) => {}
        other => panic!("expected a NotYours refusal, got {other:?}"),
    }

    // Daisy leads, so her mulligan is the one the game awaits.
    common::send(&mut members.sam_ws, &keep_opening_hand(1)).await;
    match common::recv_within(&mut members.tal_ws).await {
        ServerMessage::Applied(applied) => assert_eq!(applied.by, InvestigatorId(1)),
        other => panic!("expected Sam's answer applied, got {other:?}"),
    }
}

#[tokio::test]
async fn a_start_the_engine_rejects_is_refused_and_the_lobby_is_unchanged() {
    let mut members = tal_and_sam_at_the_gathering(NO_RANDOM_WEAKNESS).await;
    daisy_roland_agnes(&mut members).await;
    common::send(
        &mut members.tal_ws,
        &ClientMessage::SetWeakness {
            investigator: CardCode::new(ROLAND),
            choice: WeaknessChoice::Random,
        },
    )
    .await;
    let _ = recv_lobby(&mut members.tal_ws).await;
    let (_, before, _) = recv_lobby(&mut members.sam_ws).await;

    common::send(&mut members.tal_ws, &ClientMessage::Start).await;

    match recv_refusal(&mut members.tal_ws).await {
        LobbyRefusal::Seating(reason) => {
            assert!(reason.contains("weakness pool"), "{reason}");
        }
        other => panic!("expected a Seating refusal, got {other:?}"),
    }
    assert!(
        GameSession::load(
            members.pool.clone(),
            NO_RANDOM_WEAKNESS,
            &GameId::new(&members.game_id)
        )
        .await
        .expect("a load runs")
        .is_none(),
        "no game was stored"
    );

    // The lobby is still there, as it was: a give-up changes it from there.
    common::send(
        &mut members.tal_ws,
        &ClientMessage::GiveUp {
            investigator: CardCode::new(AGNES),
        },
    )
    .await;
    let (_, after, _) = recv_lobby(&mut members.sam_ws).await;
    assert_eq!(after.picks, before.picks[..2]);
    assert_eq!(after.lead, before.lead);
}

/// The next frame on `ws`, which must be a `Hello`: the creator it names and
/// its members' display names, in the order they joined.
async fn recv_hello_members(ws: &mut Client) -> (AccountId, Vec<String>) {
    match common::recv_within(ws).await {
        ServerMessage::Hello {
            creator, members, ..
        } => (creator, display_names(&members)),
        other => panic!("expected a Hello, got {other:?}"),
    }
}

/// The display names of `members`, in their order.
fn display_names(members: &[Member]) -> Vec<String> {
    members
        .iter()
        .map(|member| member.display_name.clone())
        .collect()
}

#[tokio::test]
async fn hello_names_the_creator_and_every_member_and_still_does_after_a_restart() {
    let mut members = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    daisy_roland_agnes(&mut members).await;

    common::send(&mut members.tal_ws, &ClientMessage::Start).await;

    let both = (
        members.tal,
        vec![TEST_ACCOUNT.to_string(), OTHER_ACCOUNT.to_string()],
    );
    assert_eq!(recv_hello_members(&mut members.tal_ws).await, both);
    assert_eq!(recv_hello_members(&mut members.sam_ws).await, both);

    // A second server on the same database loads the game from its rows.
    let restarted = common::spawn_server_on(members.pool.clone(), server::REGISTRIES).await;
    let mut sam_ws = common::connect_as(restarted, &members.game_id, OTHER_ACCOUNT).await;
    assert_eq!(recv_hello_members(&mut sam_ws).await, both);
}

#[tokio::test]
async fn opening_a_started_games_link_sends_members_naming_the_newcomer_to_every_connection() {
    let mut members = started_by_tal_and_sam().await;
    common::add_account(&members.pool, JO).await;

    let mut jo_ws = common::connect_as(members.addr, &members.game_id, JO).await;

    let all_three = vec![
        TEST_ACCOUNT.to_string(),
        OTHER_ACCOUNT.to_string(),
        JO.to_string(),
    ];
    assert_eq!(
        recv_hello_members(&mut jo_ws).await,
        (members.tal, all_three.clone())
    );
    for ws in [&mut members.tal_ws, &mut members.sam_ws] {
        match common::recv_within(ws).await {
            ServerMessage::Members(sent) => assert_eq!(display_names(&sent), all_three),
            other => panic!("expected Members, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn opening_a_started_games_link_again_as_a_member_sends_no_members() {
    let mut members = started_by_tal_and_sam().await;

    let mut sam_again = common::connect_as(members.addr, &members.game_id, OTHER_ACCOUNT).await;
    let _ = recv_hello(&mut sam_again).await;
    // Daisy leads, so her mulligan is the one the game awaits. Its Applied is
    // broadcast after anything the second socket's attach broadcast.
    common::send(&mut members.sam_ws, &keep_opening_hand(1)).await;

    match common::recv_within(&mut members.tal_ws).await {
        ServerMessage::Applied(applied) => assert_eq!(applied.by, InvestigatorId(1)),
        other => panic!("expected Sam's answer applied and no Members, got {other:?}"),
    }
}
