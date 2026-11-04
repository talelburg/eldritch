//! Ordering the seating and moving the lead: the creator's commands, which
//! every member sees live, and which a forged command from another member
//! can't make.

use game_core::state::CardCode;
use protocol::lobby::{Direction, LobbyRefusal};
use protocol::ClientMessage;

use super::picks::{lead, recv_refusal, seating, take_seen_by_both, tal_and_sam, Table};
use super::recv_lobby;
use crate::common::{self, Client};

const ROLAND: &str = "01001";
const DAISY: &str = "01002";
const AGNES: &str = "01004";

/// Send a `MoveSeat` of `investigator` on `ws`.
async fn move_in_seating(ws: &mut Client, investigator: &str, direction: Direction) {
    common::send(
        ws,
        &ClientMessage::MoveSeat {
            investigator: CardCode::new(investigator),
            direction,
        },
    )
    .await;
}

/// Send a `SetLead` of `investigator` on `ws`.
async fn set_lead(ws: &mut Client, investigator: &str) {
    common::send(
        ws,
        &ClientMessage::SetLead {
            investigator: CardCode::new(investigator),
        },
    )
    .await;
}

/// Tal and Sam's lobby with Sam's Daisy, then Tal's Roland and Agnes picked,
/// every `Lobby` read on both sockets.
async fn three_picked() -> Table {
    let mut table = tal_and_sam().await;
    take_seen_by_both(&mut table.sam_ws, &mut table.tal_ws, DAISY).await;
    take_seen_by_both(&mut table.tal_ws, &mut table.sam_ws, ROLAND).await;
    take_seen_by_both(&mut table.tal_ws, &mut table.sam_ws, AGNES).await;
    table
}

#[tokio::test]
async fn tals_move_and_set_lead_reach_sams_socket() {
    let Table {
        tal,
        sam,
        mut tal_ws,
        mut sam_ws,
        ..
    } = three_picked().await;

    move_in_seating(&mut tal_ws, AGNES, Direction::Earlier).await;
    let (_, view, _) = recv_lobby(&mut sam_ws).await;
    assert_eq!(seating(&view), [(DAISY, sam), (AGNES, tal), (ROLAND, tal)]);
    assert_eq!(lead(&view), Some(DAISY), "the lead stays with Daisy");
    let _ = recv_lobby(&mut tal_ws).await;

    set_lead(&mut tal_ws, ROLAND).await;
    let (_, view, _) = recv_lobby(&mut sam_ws).await;
    assert_eq!(lead(&view), Some(ROLAND));
    assert_eq!(
        seating(&view),
        [(DAISY, sam), (AGNES, tal), (ROLAND, tal)],
        "starring a lead leaves the seating alone"
    );
}

#[tokio::test]
async fn sams_move_and_set_lead_are_refused() {
    let Table {
        tal,
        sam,
        mut tal_ws,
        mut sam_ws,
        ..
    } = three_picked().await;

    move_in_seating(&mut sam_ws, DAISY, Direction::Later).await;
    assert_eq!(recv_refusal(&mut sam_ws).await, LobbyRefusal::NotCreator);
    set_lead(&mut sam_ws, DAISY).await;
    assert_eq!(recv_refusal(&mut sam_ws).await, LobbyRefusal::NotCreator);

    // Tal's next command is applied after Sam's refused ones, so a change
    // either of them made would reach Sam's socket before this one does.
    set_lead(&mut tal_ws, AGNES).await;
    let (_, view, _) = recv_lobby(&mut sam_ws).await;
    assert_eq!(
        seating(&view),
        [(DAISY, sam), (ROLAND, tal), (AGNES, tal)],
        "Sam's refused move changed nothing"
    );
    assert_eq!(lead(&view), Some(AGNES));
}

#[tokio::test]
async fn a_restart_keeps_the_seating_and_lead_the_creator_set() {
    let Table {
        tal,
        sam,
        mut tal_ws,
        mut sam_ws,
        game_id,
        pool,
    } = three_picked().await;
    move_in_seating(&mut tal_ws, DAISY, Direction::Later).await;
    let _ = recv_lobby(&mut tal_ws).await;
    let _ = recv_lobby(&mut sam_ws).await;
    set_lead(&mut tal_ws, AGNES).await;
    let _ = recv_lobby(&mut tal_ws).await;
    let _ = recv_lobby(&mut sam_ws).await;

    // A fresh server with empty rooms over the same database.
    let restarted = common::spawn_server(pool).await;
    let mut ws = common::connect(restarted, &game_id).await;
    let (_, view, _) = recv_lobby(&mut ws).await;
    assert_eq!(seating(&view), [(ROLAND, tal), (DAISY, sam), (AGNES, tal)]);
    assert_eq!(lead(&view), Some(AGNES));
}
