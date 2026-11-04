//! The lobby view: what a member sees of a game in its lobby, rendered from
//! the `Lobby` the server sent, folded into the store by the transport's own
//! decode. One file per part of the view, so each later part gets its own.
#![cfg(target_arch = "wasm32")]

mod banner;
mod header;
mod pool;

use game_core::scenario::ScenarioId;
use leptos::task;
use protocol::lobby::{Lobby, LobbyView, Member};
use protocol::{AccountId, GameId, ServerMessage};
use web::store::{self, ClientState};
use web_sys::Element;

use crate::common::{self, BoardHandle, YOU};

/// The id of the game every lobby test opens.
pub(crate) const GAME_ID: &str = "0b6f3f9e-lobby";

/// The account Sam signs in as, a member beside [`YOU`].
pub(crate) const SAM: AccountId = AccountId(2);

/// Tal's lobby of The Gathering, signed in as [`YOU`], which Sam has joined.
pub(crate) fn tals_lobby_with_sam() -> LobbyView {
    let mut lobby = Lobby::new(
        ScenarioId::new("the-gathering"),
        YOU,
        vec![Member {
            account: YOU,
            display_name: "Tal".to_string(),
        }],
    );
    lobby.join(Member {
        account: SAM,
        display_name: "Sam".to_string(),
    });
    lobby.view()
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
