//! Ordering the seating and moving the lead: the creator's commands, which
//! every member sees live, and which a forged command from another member
//! can't make.

use protocol::lobby::{Direction, LobbyRefusal};

use super::{
    daisy_roland_agnes, lead, move_in_seating, recv_lobby, recv_refusal, seating, set_lead,
    tal_and_sam, TwoMembers, AGNES, DAISY, ROLAND,
};
use crate::common;

/// Tal and Sam's lobby of the mock scenario, with [`daisy_roland_agnes`]
/// picked.
async fn three_picked() -> TwoMembers {
    let mut members = tal_and_sam().await;
    daisy_roland_agnes(&mut members).await;
    members
}

#[tokio::test]
async fn tals_move_and_set_lead_reach_sams_socket() {
    let TwoMembers {
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
    let TwoMembers {
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
    let TwoMembers {
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
