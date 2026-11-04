//! Start: the creator seats the picks, and every connected member moves to
//! the board, each account answering only for its own investigators.
//!
//! These lobbies are of The Gathering, served the production registries, so
//! Start seats real investigators with their default decks.

use game_core::action::{InputResponse, PlayerAction};
use game_core::card_registry::CardRegistry;
use game_core::engine::Registries;
use game_core::state::investigator::InvestigatorId;
use game_core::state::CardCode;
use protocol::lobby::{LobbyRefusal, LobbyView};
use protocol::{AccountId, Assignment, ClientMessage, GameId, LogPosition, Refusal, ServerMessage};
use scenarios::the_gathering::ID as GATHERING;
use server::session::GameSession;
use sqlx::SqlitePool;

use super::picks::{recv_refusal, take};
use super::{create_lobby_of, recv_lobby};
use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT};

const ROLAND: &str = "01001";
const DAISY: &str = "01002";
const AGNES: &str = "01004";

/// Silver Twilight Acolyte 01102: Roland's default deck's fixed weakness.
const ROLANDS_FIXED_WEAKNESS: &str = "01102";

/// The production card registry, but with Roland's fixed weakness
/// unimplemented, so the engine refuses to seat Roland's default deck.
const ROLANDS_WEAKNESS_UNIMPLEMENTED: CardRegistry = CardRegistry {
    is_playable: |code| {
        code.as_str() != ROLANDS_FIXED_WEAKNESS && (cards::REGISTRY.is_playable)(code)
    },
    ..cards::REGISTRY
};

/// The production registries, but with [`ROLANDS_WEAKNESS_UNIMPLEMENTED`].
const ROLAND_UNSEATABLE: Registries = Registries {
    cards: &ROLANDS_WEAKNESS_UNIMPLEMENTED,
    ..server::REGISTRIES
};

/// Tal's lobby of The Gathering, served against `registries`, open on Tal's
/// socket and on Sam's, which joined it. Both sockets have read every `Lobby`
/// sent so far.
struct Table {
    tal: AccountId,
    sam: AccountId,
    tal_ws: Client,
    sam_ws: Client,
    game_id: String,
    pool: SqlitePool,
}

async fn tal_and_sam_at_the_gathering(registries: Registries) -> Table {
    let pool = common::memory_pool().await;
    let tal = common::test_account(&pool).await;
    let sam = common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server_on(pool.clone(), registries).await;
    let game_id = create_lobby_of(addr, TEST_ACCOUNT, GATHERING).await;
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

/// `ws` takes `investigator`, and both sockets read the `Lobby` it brings.
/// Returns the view.
async fn take_seen_by_both(ws: &mut Client, other: &mut Client, investigator: &str) -> LobbyView {
    take(ws, investigator).await;
    let _ = recv_lobby(ws).await;
    recv_lobby(other).await.1
}

/// Sam's Daisy, then Tal's Roland and Agnes: Daisy leads, since she was
/// picked first.
async fn daisy_roland_agnes(table: &mut Table) -> LobbyView {
    take_seen_by_both(&mut table.sam_ws, &mut table.tal_ws, DAISY).await;
    take_seen_by_both(&mut table.tal_ws, &mut table.sam_ws, ROLAND).await;
    take_seen_by_both(&mut table.tal_ws, &mut table.sam_ws, AGNES).await
}

/// The next frame on `ws`, which must be a `Hello`: its `you` and assignment.
async fn recv_hello(ws: &mut Client) -> (AccountId, Assignment) {
    match common::recv_within(ws).await {
        ServerMessage::Hello { you, assigned, .. } => (you, assigned),
        other => panic!("expected a Hello, got {other:?}"),
    }
}

/// Keep the opening hand of `investigator`, answering the mulligan prompt at
/// the start of the log.
fn keep_opening_hand(investigator: u32) -> ClientMessage {
    ClientMessage::Submit {
        prompt: LogPosition(0),
        action: PlayerAction::ResolveInput {
            investigator: InvestigatorId(investigator),
            response: InputResponse::PickMultiple { selected: vec![] },
        },
    }
}

#[tokio::test]
async fn only_the_creator_may_start() {
    let mut table = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    take_seen_by_both(&mut table.sam_ws, &mut table.tal_ws, DAISY).await;

    common::send(&mut table.sam_ws, &ClientMessage::Start).await;

    assert_eq!(
        recv_refusal(&mut table.sam_ws).await,
        LobbyRefusal::NotCreator
    );
}

#[tokio::test]
async fn start_moves_every_member_to_the_board_seated_as_the_lobby_was() {
    let mut table = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    daisy_roland_agnes(&mut table).await;

    common::send(&mut table.tal_ws, &ClientMessage::Start).await;

    let seated = Assignment::from_iter([
        (InvestigatorId(1), table.sam),
        (InvestigatorId(2), table.tal),
        (InvestigatorId(3), table.tal),
    ]);
    assert_eq!(
        recv_hello(&mut table.tal_ws).await,
        (table.tal, seated.clone())
    );
    assert_eq!(
        recv_hello(&mut table.sam_ws).await,
        (table.sam, seated.clone())
    );

    // What was stored, read back as a restart would.
    let stored = GameSession::load(
        table.pool.clone(),
        server::REGISTRIES,
        &GameId::new(&table.game_id),
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
    assert_eq!(names, ["Daisy Walker", "Roland Banks", "Agnes Baker"]);
    assert_eq!(state.lead, Some(InvestigatorId(1)), "Daisy leads");
    assert_eq!(stored.live().assignment(), &seated);

    // The roster is frozen.
    take(&mut table.sam_ws, "01005").await;
    assert_eq!(recv_refusal(&mut table.sam_ws).await, LobbyRefusal::Started);
}

#[tokio::test]
async fn after_start_sam_answers_for_daisy_and_is_refused_for_roland() {
    let mut table = tal_and_sam_at_the_gathering(server::REGISTRIES).await;
    daisy_roland_agnes(&mut table).await;
    common::send(&mut table.tal_ws, &ClientMessage::Start).await;
    let _ = recv_hello(&mut table.tal_ws).await;
    let _ = recv_hello(&mut table.sam_ws).await;

    // Roland is investigator 2, Tal's.
    common::send(&mut table.sam_ws, &keep_opening_hand(2)).await;
    match common::recv_within(&mut table.sam_ws).await {
        ServerMessage::Rejected(Refusal::NotYours) => {}
        other => panic!("expected a NotYours refusal, got {other:?}"),
    }

    // Daisy leads, so her mulligan is the one the game awaits.
    common::send(&mut table.sam_ws, &keep_opening_hand(1)).await;
    match common::recv_within(&mut table.tal_ws).await {
        ServerMessage::Applied(applied) => assert_eq!(applied.by, InvestigatorId(1)),
        other => panic!("expected Sam's answer applied, got {other:?}"),
    }
}

#[tokio::test]
async fn a_start_the_engine_rejects_is_refused_and_the_lobby_is_unchanged() {
    let mut table = tal_and_sam_at_the_gathering(ROLAND_UNSEATABLE).await;
    let before = daisy_roland_agnes(&mut table).await;

    common::send(&mut table.tal_ws, &ClientMessage::Start).await;

    match recv_refusal(&mut table.tal_ws).await {
        LobbyRefusal::Seating(reason) => {
            assert!(reason.contains(ROLANDS_FIXED_WEAKNESS), "{reason}");
        }
        other => panic!("expected a Seating refusal, got {other:?}"),
    }
    assert!(
        GameSession::load(
            table.pool.clone(),
            ROLAND_UNSEATABLE,
            &GameId::new(&table.game_id)
        )
        .await
        .expect("a load runs")
        .is_none(),
        "no game was stored"
    );

    // The lobby is still there, as it was: a give-up changes it from there.
    common::send(
        &mut table.tal_ws,
        &ClientMessage::GiveUp {
            investigator: CardCode::new(AGNES),
        },
    )
    .await;
    let (_, after, _) = recv_lobby(&mut table.sam_ws).await;
    assert_eq!(after.picks, before.picks[..2]);
    assert_eq!(after.lead, before.lead);
}
