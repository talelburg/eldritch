//! The lobby view: what a member sees of a game in its lobby, rendered from
//! the `Lobby` the server sent, folded into the store by the transport's own
//! decode. One file per part of the view, so each later part gets its own.
#![cfg(target_arch = "wasm32")]

mod banner;
mod header;
mod picks;
mod pool;
mod seating;
mod seating_controls;
mod start;
mod toast;
mod weakness;

use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use leptos::task;
use protocol::lobby::{Lobby, LobbyView, Member};
use protocol::{AccountId, ClientMessage, GameId, ServerMessage};
use wasm_bindgen::JsCast;
use web::store::{self, ClientState};
use web_sys::{Element, HtmlElement};

use crate::common::{self, BoardHandle, YOU};

/// The id of the game every lobby test opens.
pub(crate) const GAME_ID: &str = "0b6f3f9e-lobby";

/// The account Sam signs in as, a member beside [`YOU`].
pub(crate) const SAM: AccountId = AccountId(2);

/// Tal's lobby of The Gathering, signed in as [`YOU`], which Sam has joined.
pub(crate) fn tals_lobby_with_sam() -> LobbyView {
    tals_lobby_with_picks(&[])
}

/// [`tals_lobby_with_sam`], with `picks` taken in order: each an
/// investigator's card code and the member who takes it.
pub(crate) fn tals_lobby_with_picks(picks: &[(&str, AccountId)]) -> LobbyView {
    let mut lobby = Lobby::new(
        ScenarioId::new("the-gathering"),
        YOU,
        vec![
            Member {
                account: YOU,
                display_name: "Tal".to_string(),
            },
            Member {
                account: SAM,
                display_name: "Sam".to_string(),
            },
        ],
    );
    for (investigator, account) in picks {
        lobby
            .take(*account, CardCode::new(*investigator))
            .expect("each test pick is free");
    }
    lobby.view()
}

/// Click the one `sel` under `handle`'s root.
pub(crate) fn click(handle: &BoardHandle, sel: &str) {
    one(handle, sel)
        .dyn_into::<HtmlElement>()
        .expect("an HTML element")
        .click();
}

/// The frame the lobby sent, which must be the only one.
pub(crate) fn sent(handle: &mut BoardHandle) -> ClientMessage {
    let frame = handle.rx.try_recv().expect("a frame was sent");
    assert!(handle.rx.try_recv().is_err(), "only one frame was sent");
    frame
}

/// Mount the app's main column at the game [`GAME_ID`], signed in as `you`,
/// after the `Lobby` frame carrying `view` and `joined` arrived.
pub(crate) async fn mount_lobby(view: LobbyView, you: AccountId, joined: bool) -> BoardHandle {
    let mut client = ClientState {
        game_id: Some(GameId::new(GAME_ID)),
        ..ClientState::default()
    };
    let frame = serde_json::to_string(&ServerMessage::Lobby {
        build: protocol::build_id().to_string(),
        you,
        view,
        joined,
    })
    .expect("a Lobby serializes");
    store::receive(&mut client, &frame);
    let handle = common::mount_board(client, &cards::TEST_REGISTRY).await;
    task::tick().await;
    handle
}

/// The one `sel` under `handle`'s root.
pub(crate) fn one(handle: &BoardHandle, sel: &str) -> Element {
    handle
        .root
        .query_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("the lobby renders {sel}"))
}

/// The text of every `sel` under `handle`'s root, in document order.
pub(crate) fn texts(handle: &BoardHandle, sel: &str) -> Vec<String> {
    let nodes = handle.root.query_selector_all(sel).expect("query");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .map(|node| node.text_content().unwrap_or_default())
        .collect()
}
