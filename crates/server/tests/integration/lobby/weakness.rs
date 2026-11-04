//! A pick's weakness choice: its member sets it, every member sees it, and a
//! restart keeps it.

use cards::decks::WeaknessChoice;
use game_core::state::CardCode;
use protocol::lobby::{LobbyRefusal, LobbyView};
use protocol::ClientMessage;

use super::{
    recv_lobby, recv_refusal, take, take_seen_by_both, tal_and_sam, TwoMembers, DAISY, ROLAND,
};
use crate::common::{self, Client};

/// Send a `SetWeakness` of `investigator` to `choice` on `ws`.
async fn set_weakness(ws: &mut Client, investigator: &str, choice: WeaknessChoice) {
    common::send(
        ws,
        &ClientMessage::SetWeakness {
            investigator: CardCode::new(investigator),
            choice,
        },
    )
    .await;
}

/// The weakness choice of `investigator`'s pick in `view`.
fn weakness_of(view: &LobbyView, investigator: &str) -> WeaknessChoice {
    view.picks
        .iter()
        .find(|pick| pick.investigator.as_str() == investigator)
        .unwrap_or_else(|| panic!("{investigator} is picked"))
        .weakness
}

#[tokio::test]
async fn sams_set_weakness_reaches_tals_socket() {
    let TwoMembers {
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    let view = take_seen_by_both(&mut sam_ws, &mut tal_ws, DAISY).await;
    assert_eq!(
        weakness_of(&view, DAISY),
        WeaknessChoice::Fixed,
        "a new pick's weakness is fixed"
    );

    set_weakness(&mut sam_ws, DAISY, WeaknessChoice::Random).await;

    let (_, view, _) = recv_lobby(&mut tal_ws).await;
    assert_eq!(weakness_of(&view, DAISY), WeaknessChoice::Random);
}

#[tokio::test]
async fn setting_the_weakness_of_another_members_pick_is_refused() {
    let TwoMembers {
        mut tal_ws,
        mut sam_ws,
        ..
    } = tal_and_sam().await;
    take_seen_by_both(&mut sam_ws, &mut tal_ws, DAISY).await;

    set_weakness(&mut tal_ws, DAISY, WeaknessChoice::Random).await;
    assert_eq!(
        recv_refusal(&mut tal_ws).await,
        LobbyRefusal::NotYourPick {
            investigator: CardCode::new(DAISY),
        }
    );

    // Tal's next command is applied after the refused one, so a change the
    // refusal made would reach Sam's socket before this one does.
    take(&mut tal_ws, ROLAND).await;
    let (_, view, _) = recv_lobby(&mut sam_ws).await;
    assert_eq!(
        weakness_of(&view, DAISY),
        WeaknessChoice::Fixed,
        "the refused SetWeakness changed nothing"
    );
}

#[tokio::test]
async fn a_restart_mid_lobby_keeps_each_picks_weakness() {
    let TwoMembers {
        mut tal_ws,
        mut sam_ws,
        game_id,
        pool,
        ..
    } = tal_and_sam().await;
    take_seen_by_both(&mut sam_ws, &mut tal_ws, DAISY).await;
    set_weakness(&mut sam_ws, DAISY, WeaknessChoice::Random).await;
    let _ = recv_lobby(&mut sam_ws).await;
    let _ = recv_lobby(&mut tal_ws).await;
    // A later change stores every pick again, Daisy's choice included.
    take_seen_by_both(&mut tal_ws, &mut sam_ws, ROLAND).await;

    // A fresh server with empty rooms over the same database.
    let restarted = common::spawn_server(pool).await;
    let mut ws = common::connect(restarted, &game_id).await;
    let (_, view, _) = recv_lobby(&mut ws).await;
    assert_eq!(weakness_of(&view, DAISY), WeaknessChoice::Random);
    assert_eq!(weakness_of(&view, ROLAND), WeaknessChoice::Fixed);
}
