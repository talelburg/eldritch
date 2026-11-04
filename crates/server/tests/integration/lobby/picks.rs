//! Taking and giving up picks: every member sees each change live, and a
//! refused command is answered to its sender alone.

use game_core::state::CardCode;
use protocol::lobby::LobbyRefusal;
use protocol::ClientMessage;

use super::{
    lead, recv_lobby, recv_refusal, seating, take, take_seen_by_both, tal_and_sam, TwoMembers,
    AGNES, DAISY, ROLAND, SKIDS, WENDY,
};
use crate::common::{self, Client};

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

#[tokio::test]
async fn sams_take_reaches_tals_socket() {
    let TwoMembers {
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
    let TwoMembers {
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
    let TwoMembers {
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    for investigator in [ROLAND, DAISY, SKIDS, AGNES] {
        take_seen_by_both(&mut tal_ws, &mut sam_ws, investigator).await;
    }

    take(&mut sam_ws, WENDY).await;
    assert_eq!(recv_refusal(&mut sam_ws).await, LobbyRefusal::LobbyFull);
}

/// The pool is the default decks' investigators: Start seats each pick with
/// its default deck.
#[tokio::test]
async fn a_take_of_an_investigator_with_no_default_deck_is_refused() {
    let TwoMembers { mut sam_ws, .. } = tal_and_sam().await;

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
    let TwoMembers {
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
    let TwoMembers {
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
    let TwoMembers {
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
