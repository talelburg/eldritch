//! Leave and Remove: a member steps out of the lobby, or the creator removes
//! one. Either drops the member and their picks, closes every socket of theirs
//! with [`CLOSE_NO_LONGER_A_MEMBER`], and takes the game off their "Your
//! games" listing. Neither is a ban: opening the link again rejoins.

use protocol::lobby::LobbyRefusal;
use protocol::{AccountId, ClientMessage, CLOSE_NO_LONGER_A_MEMBER};

use super::{
    assert_still_open, daisy_roland_agnes, lead, listed_for, member_names, recv_lobby,
    recv_refusal, seating, tal_and_sam, TwoMembers, AGNES, DAISY, ROLAND,
};
use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT};

/// [`tal_and_sam`] with Sam's Daisy leading, then Tal's Roland and Agnes, and
/// a second socket of Sam's open on the lobby, which has read its `Lobby`.
async fn sam_in_two_tabs() -> (TwoMembers, Client) {
    let mut members = tal_and_sam().await;
    daisy_roland_agnes(&mut members).await;
    let mut sam_second = common::connect_as(members.addr, &members.game_id, OTHER_ACCOUNT).await;
    let (_, _, joined) = recv_lobby(&mut sam_second).await;
    assert!(!joined, "Sam is already a member");
    (members, sam_second)
}

/// What Tal sees once Sam has gone, by either route: Sam's Daisy is free, the
/// lead passes to Roland, now first in the seating, and Sam's sockets close.
/// Sam's listing drops the game, and opening the link again makes Sam a
/// member again.
async fn assert_sam_is_gone(mut members: TwoMembers, mut sam_second: Client) {
    let (_, view, _) = recv_lobby(&mut members.tal_ws).await;
    assert_eq!(member_names(&view), [TEST_ACCOUNT]);
    assert_eq!(
        seating(&view),
        [(ROLAND, members.tal), (AGNES, members.tal)],
        "Sam's Daisy is free"
    );
    assert_eq!(lead(&view), Some(ROLAND), "Daisy's lead passes on");

    common::assert_closed_with(&mut members.sam_ws, CLOSE_NO_LONGER_A_MEMBER).await;
    common::assert_closed_with(&mut sam_second, CLOSE_NO_LONGER_A_MEMBER).await;

    assert!(
        !listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await,
        "off Sam's list"
    );
    assert!(
        listed_for(members.addr, &members.game_id, TEST_ACCOUNT).await,
        "still on Tal's"
    );

    let mut sam_again = common::connect_as(members.addr, &members.game_id, OTHER_ACCOUNT).await;
    let (you, view, joined) = recv_lobby(&mut sam_again).await;
    assert_eq!(you, members.sam);
    assert!(joined, "opening the link made Sam a member again");
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert!(
        !view
            .picks
            .iter()
            .any(|pick| pick.investigator.as_str() == DAISY),
        "rejoining doesn't bring Sam's pick back"
    );
    let (_, view, _) = recv_lobby(&mut members.tal_ws).await;
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert!(
        listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await,
        "back on Sam's list"
    );
}

#[tokio::test]
async fn a_member_leaving_frees_their_picks_passes_the_lead_and_closes_their_sockets() {
    let (mut members, sam_second) = sam_in_two_tabs().await;

    common::send(&mut members.sam_ws, &ClientMessage::Leave).await;

    assert_sam_is_gone(members, sam_second).await;
}

#[tokio::test]
async fn the_creator_removing_a_member_frees_their_picks_and_closes_their_sockets() {
    let (mut members, sam_second) = sam_in_two_tabs().await;

    common::send(
        &mut members.tal_ws,
        &ClientMessage::Remove {
            member: members.sam,
        },
    )
    .await;

    assert_sam_is_gone(members, sam_second).await;
}

#[tokio::test]
async fn a_members_remove_and_the_creators_leave_are_refused_and_change_nothing() {
    let (mut members, mut sam_second) = sam_in_two_tabs().await;

    common::send(
        &mut members.sam_ws,
        &ClientMessage::Remove {
            member: members.tal,
        },
    )
    .await;
    assert_eq!(
        recv_refusal(&mut members.sam_ws).await,
        LobbyRefusal::NotCreator
    );

    common::send(&mut members.tal_ws, &ClientMessage::Leave).await;
    assert_eq!(
        recv_refusal(&mut members.tal_ws).await,
        LobbyRefusal::CreatorStays
    );
    common::send(
        &mut members.tal_ws,
        &ClientMessage::Remove {
            member: members.tal,
        },
    )
    .await;
    assert_eq!(
        recv_refusal(&mut members.tal_ws).await,
        LobbyRefusal::CreatorStays
    );
    common::send(
        &mut members.tal_ws,
        &ClientMessage::Remove {
            member: AccountId(99),
        },
    )
    .await;
    assert_eq!(
        recv_refusal(&mut members.tal_ws).await,
        LobbyRefusal::NotAMember
    );

    // Nothing was broadcast, and a fresh socket reads the lobby as it was.
    assert_still_open(&mut sam_second).await;
    let mut tal_again = common::connect(members.addr, &members.game_id).await;
    let (_, view, _) = recv_lobby(&mut tal_again).await;
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert_eq!(
        seating(&view),
        [
            (DAISY, members.sam),
            (ROLAND, members.tal),
            (AGNES, members.tal)
        ]
    );
    assert_eq!(lead(&view), Some(DAISY));
    assert!(listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await);
}
