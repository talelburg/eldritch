//! Reassignment: the creator, or the account holding an investigator, moves
//! it to another member while the game is in progress, and the new holder
//! answers the prompt already waiting.
//!
//! Each game here is [`started_by_tal_and_sam`]: Sam holds Daisy (1), whose
//! mulligan the game awaits, and Tal, the creator, holds Roland (2) and
//! Agnes (3).

use game_core::state::investigator::InvestigatorId;
use protocol::{AccountId, Assignment, ClientMessage, ReassignRefusal, Refusal, ServerMessage};

use super::start::{keep_opening_hand, recv_hello, started_by_tal_and_sam, JO};
use crate::common::{self, Client, OTHER_ACCOUNT};

/// Daisy Walker, Sam's investigator, whose mulligan the game awaits.
const DAISY: InvestigatorId = InvestigatorId(1);
/// Roland Banks, Tal's.
const ROLAND: InvestigatorId = InvestigatorId(2);
/// Agnes Baker, Tal's.
const AGNES: InvestigatorId = InvestigatorId(3);

/// Send a reassignment of `investigator` to `to` on `ws`.
async fn reassign(ws: &mut Client, investigator: InvestigatorId, to: AccountId) {
    common::send(ws, &ClientMessage::Reassign { investigator, to }).await;
}

/// The next frame on `ws`, which must be an `Assigned`: who reassigned which
/// investigator, and the whole new assignment.
async fn recv_assigned(ws: &mut Client) -> (AccountId, InvestigatorId, Assignment) {
    match common::recv_within(ws).await {
        ServerMessage::Assigned {
            by,
            investigator,
            assigned,
        } => (by, investigator, assigned),
        other => panic!("expected Assigned, got {other:?}"),
    }
}

/// The next frame on `ws`, which must refuse a reassignment: why.
async fn recv_reassign_refusal(ws: &mut Client) -> ReassignRefusal {
    match common::recv_within(ws).await {
        ServerMessage::Rejected(Refusal::Reassign(reason)) => reason,
        other => panic!("expected a refused reassignment, got {other:?}"),
    }
}

/// Every investigator held by `tal`, as reassigning Daisy to Tal leaves them.
fn all_to(tal: AccountId) -> Assignment {
    Assignment::all_to(tal, [DAISY, ROLAND, AGNES])
}

#[tokio::test]
async fn sam_reassigns_daisy_to_tal_who_answers_the_mulligan_already_waiting() {
    let mut members = started_by_tal_and_sam().await;

    reassign(&mut members.sam_ws, DAISY, members.tal).await;

    let told = (members.sam, DAISY, all_to(members.tal));
    assert_eq!(recv_assigned(&mut members.sam_ws).await, told);
    assert_eq!(recv_assigned(&mut members.tal_ws).await, told);

    common::send(&mut members.sam_ws, &keep_opening_hand(1)).await;
    match common::recv_within(&mut members.sam_ws).await {
        ServerMessage::Rejected(Refusal::NotYours) => {}
        other => panic!("expected Sam's answer refused as NotYours, got {other:?}"),
    }

    common::send(&mut members.tal_ws, &keep_opening_hand(1)).await;
    match common::recv_within(&mut members.tal_ws).await {
        ServerMessage::Applied(applied) => assert_eq!(applied.by, DAISY),
        other => panic!("expected Tal's answer for Daisy applied, got {other:?}"),
    }
}

/// Tal's reassignment and Sam's answer for Daisy race. Whichever the room
/// takes first, Sam never reads a refusal before the `Assigned` that caused
/// it: either Sam's answer applied and then Daisy moved, or Daisy moved and
/// then Sam's answer was refused.
#[tokio::test]
async fn an_answer_racing_the_reassignment_is_refused_only_after_assigned() {
    let mut members = started_by_tal_and_sam().await;

    reassign(&mut members.tal_ws, DAISY, members.tal).await;
    common::send(&mut members.sam_ws, &keep_opening_hand(1)).await;

    let first = common::recv_within(&mut members.sam_ws).await;
    let second = common::recv_within(&mut members.sam_ws).await;
    match (first, second) {
        (ServerMessage::Assigned { .. }, ServerMessage::Rejected(Refusal::NotYours))
        | (ServerMessage::Applied(_), ServerMessage::Assigned { .. }) => {}
        other => panic!("expected Assigned before any refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn the_creator_reassigns_an_investigator_another_account_holds() {
    let mut members = started_by_tal_and_sam().await;

    reassign(&mut members.tal_ws, DAISY, members.tal).await;

    let told = (members.tal, DAISY, all_to(members.tal));
    assert_eq!(recv_assigned(&mut members.tal_ws).await, told);
    assert_eq!(recv_assigned(&mut members.sam_ws).await, told);
}

#[tokio::test]
async fn a_holder_may_not_reassign_another_accounts_investigator() {
    let mut members = started_by_tal_and_sam().await;

    reassign(&mut members.sam_ws, ROLAND, members.sam).await;

    assert_eq!(
        recv_reassign_refusal(&mut members.sam_ws).await,
        ReassignRefusal::NotHolder
    );
}

#[tokio::test]
async fn a_member_holding_no_investigator_is_refused() {
    let members = started_by_tal_and_sam().await;
    let jo = common::add_account(&members.pool, JO).await;
    let mut jo_ws = common::connect_as(members.addr, &members.game_id, JO).await;
    let _ = recv_hello(&mut jo_ws).await;

    reassign(&mut jo_ws, DAISY, jo).await;

    assert_eq!(
        recv_reassign_refusal(&mut jo_ws).await,
        ReassignRefusal::NotHolder
    );
}

#[tokio::test]
async fn a_reassignment_to_an_account_that_is_not_a_member_is_refused() {
    let mut members = started_by_tal_and_sam().await;
    // Jo is on the allowlist but never opened the game's link.
    let jo = common::add_account(&members.pool, JO).await;

    reassign(&mut members.tal_ws, AGNES, jo).await;

    assert_eq!(
        recv_reassign_refusal(&mut members.tal_ws).await,
        ReassignRefusal::NotAMember
    );
}

#[tokio::test]
async fn a_reassignment_to_the_current_holder_is_refused() {
    let mut members = started_by_tal_and_sam().await;

    reassign(&mut members.tal_ws, DAISY, members.sam).await;

    assert_eq!(
        recv_reassign_refusal(&mut members.tal_ws).await,
        ReassignRefusal::AlreadyHolds
    );
}

#[tokio::test]
async fn a_reassignment_survives_a_restart() {
    let mut members = started_by_tal_and_sam().await;
    reassign(&mut members.sam_ws, DAISY, members.tal).await;
    let _ = recv_assigned(&mut members.sam_ws).await;

    // A second server on the same database loads the game from its rows.
    let restarted = common::spawn_server_on(members.pool.clone(), server::REGISTRIES).await;
    let mut sam_ws = common::connect_as(restarted, &members.game_id, OTHER_ACCOUNT).await;

    assert_eq!(
        recv_hello(&mut sam_ws).await,
        (members.sam, all_to(members.tal))
    );
}

#[tokio::test]
async fn jo_who_opened_the_link_after_start_can_be_reassigned_to() {
    let mut members = started_by_tal_and_sam().await;
    let jo = common::add_account(&members.pool, JO).await;
    let mut jo_ws = common::connect_as(members.addr, &members.game_id, JO).await;
    let _ = recv_hello(&mut jo_ws).await;
    // The Members naming Jo.
    let _ = common::recv_within(&mut members.tal_ws).await;

    reassign(&mut members.tal_ws, AGNES, jo).await;

    let assigned =
        Assignment::from_iter([(DAISY, members.sam), (ROLAND, members.tal), (AGNES, jo)]);
    assert_eq!(
        recv_assigned(&mut jo_ws).await,
        (members.tal, AGNES, assigned)
    );
}

#[tokio::test]
async fn a_reassignment_in_the_lobby_is_refused_as_not_started() {
    let mut members = super::tal_and_sam().await;

    reassign(&mut members.tal_ws, DAISY, members.sam).await;

    match common::recv_within(&mut members.tal_ws).await {
        ServerMessage::Rejected(Refusal::NotStarted) => {}
        other => panic!("expected a NotStarted refusal, got {other:?}"),
    }
}
