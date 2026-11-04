//! Taking and giving up picks: every member sees each change live, and a
//! refused command is answered to its sender alone.

use game_core::state::CardCode;
use protocol::lobby::{LobbyRefusal, LobbyView, Pick};
use protocol::{AccountId, ClientMessage, ServerMessage};

use super::{create_lobby, recv_lobby};
use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT};

const ROLAND: &str = "01001";
const DAISY: &str = "01002";
const SKIDS: &str = "01003";
const AGNES: &str = "01004";
const WENDY: &str = "01005";

/// Send a `Take` of `investigator` on `ws`.
pub(crate) async fn take(ws: &mut Client, investigator: &str) {
    common::send(
        ws,
        &ClientMessage::Take {
            investigator: CardCode::new(investigator),
        },
    )
    .await;
}

/// Send a `GiveUp` of `investigator` on `ws`.
async fn give_up(ws: &mut Client, investigator: &str) {
    common::send(
        ws,
        &ClientMessage::GiveUp {
            investigator: CardCode::new(investigator),
        },
    )
    .await;
}

/// The next frame on `ws`, which must be a `LobbyRefused`: its reason.
pub(crate) async fn recv_refusal(ws: &mut Client) -> LobbyRefusal {
    match common::recv_within(ws).await {
        ServerMessage::LobbyRefused { reason } => reason,
        other => panic!("expected a LobbyRefused, got {other:?}"),
    }
}

/// A view's picks as `(investigator, account)`, in seating order.
fn seating(view: &LobbyView) -> Vec<(&str, AccountId)> {
    view.picks
        .iter()
        .map(
            |Pick {
                 investigator,
                 account,
             }| (investigator.as_str(), *account),
        )
        .collect()
}

/// The lead a view names, by card code.
fn lead(view: &LobbyView) -> Option<&str> {
    view.lead.as_ref().map(CardCode::as_str)
}

/// Tal's lobby, open on Tal's socket and on Sam's, which joined it. Both
/// sockets have read every `Lobby` sent so far.
struct Table {
    tal: AccountId,
    sam: AccountId,
    tal_ws: Client,
    sam_ws: Client,
    game_id: String,
    pool: sqlx::SqlitePool,
}

async fn tal_and_sam() -> Table {
    let pool = common::memory_pool().await;
    let tal = common::test_account(&pool).await;
    let sam = common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool.clone()).await;
    let game_id = create_lobby(addr, TEST_ACCOUNT).await;
    let mut tal_ws = common::connect(addr, &game_id).await;
    let _ = recv_lobby(&mut tal_ws).await;
    let mut sam_ws = common::connect_as(addr, &game_id, OTHER_ACCOUNT).await;
    let _ = recv_lobby(&mut sam_ws).await;
    let _ = recv_lobby(&mut tal_ws).await;
    Table {
        tal,
        sam,
        tal_ws,
        sam_ws,
        game_id,
        pool,
    }
}

/// Send a `Take` of `investigator` on `ws`. and read the `Lobby` it brings to
/// both sockets. Returns the view the other socket was sent.
async fn take_seen_by_both(ws: &mut Client, other: &mut Client, investigator: &str) -> LobbyView {
    take(ws, investigator).await;
    let _ = recv_lobby(ws).await;
    recv_lobby(other).await.1
}

#[tokio::test]
async fn sams_take_reaches_tals_socket() {
    let Table {
        sam,
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;

    take(&mut sam_ws, DAISY).await;

    let (_, view, joined) = recv_lobby(&mut tal_ws).await;
    assert_eq!(seating(&view), [(DAISY, sam)], "Daisy is Sam's");
    assert_eq!(lead(&view), Some(DAISY), "the first pick leads");
    assert!(!joined);
    let (_, view, _) = recv_lobby(&mut sam_ws).await;
    assert_eq!(seating(&view), [(DAISY, sam)]);
}

#[tokio::test]
async fn a_take_of_an_investigator_already_held_is_refused_to_its_sender_alone() {
    let Table {
        tal,
        sam,
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, ROLAND).await;

    take(&mut sam_ws, ROLAND).await;
    assert_eq!(
        recv_refusal(&mut sam_ws).await,
        LobbyRefusal::Taken {
            investigator: CardCode::new(ROLAND),
            by: tal,
        }
    );

    // Sam's next command is applied after the refused one, so a refusal sent
    // to Tal would reach Tal's socket before this change does.
    take(&mut sam_ws, DAISY).await;
    let (_, view, _) = recv_lobby(&mut tal_ws).await;
    assert_eq!(
        seating(&view),
        [(ROLAND, tal), (DAISY, sam)],
        "the refused Take changed nothing"
    );
}

#[tokio::test]
async fn a_fifth_take_is_refused() {
    let Table {
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    for investigator in [ROLAND, DAISY, SKIDS, AGNES] {
        take_seen_by_both(&mut tal_ws, &mut sam_ws, investigator).await;
    }

    take(&mut sam_ws, WENDY).await;
    assert_eq!(recv_refusal(&mut sam_ws).await, LobbyRefusal::TableFull);
}

/// The pool is the default decks' investigators: Start seats each pick with
/// its default deck.
#[tokio::test]
async fn a_take_of_an_investigator_with_no_default_deck_is_refused() {
    let Table { mut sam_ws, .. } = tal_and_sam().await;

    // .45 Automatic: a card, but no investigator with a default deck.
    take(&mut sam_ws, "01016").await;
    assert_eq!(
        recv_refusal(&mut sam_ws).await,
        LobbyRefusal::NotInPool {
            investigator: CardCode::new("01016"),
        }
    );
}

#[tokio::test]
async fn giving_up_the_lead_closes_the_gap_and_passes_the_lead_on() {
    let Table {
        tal,
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    take_seen_by_both(&mut sam_ws, &mut tal_ws, DAISY).await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, ROLAND).await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, AGNES).await;

    give_up(&mut sam_ws, DAISY).await;

    let (_, view, _) = recv_lobby(&mut tal_ws).await;
    assert_eq!(seating(&view), [(ROLAND, tal), (AGNES, tal)]);
    assert_eq!(lead(&view), Some(ROLAND), "the new first position leads");
}

#[tokio::test]
async fn giving_up_another_members_pick_is_refused() {
    let Table {
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, ROLAND).await;

    give_up(&mut sam_ws, ROLAND).await;
    assert_eq!(
        recv_refusal(&mut sam_ws).await,
        LobbyRefusal::NotYourPick {
            investigator: CardCode::new(ROLAND),
        }
    );
}

#[tokio::test]
async fn a_restart_mid_lobby_keeps_the_picks_their_seating_and_the_lead() {
    let Table {
        tal,
        sam,
        mut tal_ws,
        mut sam_ws,
        game_id,
        pool,
    } = tal_and_sam().await;
    take_seen_by_both(&mut sam_ws, &mut tal_ws, DAISY).await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, ROLAND).await;
    take_seen_by_both(&mut tal_ws, &mut sam_ws, AGNES).await;
    // Daisy's give-up passes the lead to Roland and shifts Agnes up a place,
    // so the stored positions and lead are both ones a command rewrote.
    give_up(&mut sam_ws, DAISY).await;
    let _ = recv_lobby(&mut sam_ws).await;
    let _ = recv_lobby(&mut tal_ws).await;
    take_seen_by_both(&mut sam_ws, &mut tal_ws, WENDY).await;

    // A fresh server with empty rooms over the same database.
    let restarted = common::spawn_server(pool).await;
    let mut ws = common::connect(restarted, &game_id).await;
    let (_, view, _) = recv_lobby(&mut ws).await;
    assert_eq!(seating(&view), [(ROLAND, tal), (AGNES, tal), (WENDY, sam)]);
    assert_eq!(lead(&view), Some(ROLAND));
}
