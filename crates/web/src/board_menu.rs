//! The board's ⋯ menu: Reassign `<investigator>`, over the members each may go
//! to, then Delete for the creator.
//!
//! Its entries are [`ClientState::reassignments`]: the creator's cover every
//! investigator, a holder's only their own, a viewer has none, and a finished
//! game offers none. Picking a member sends the reassignment; the board
//! changes only once the `Assigned` it causes arrives. wasm-only — it sends
//! through the wasm-only `OutboundTx`.

use leptos::prelude::*;
use protocol::ClientMessage;

use crate::delete_game;
use crate::more_menu::{MenuEntry, MoreMenu};
use crate::store::{self, ClientState, StoreSignal};
use crate::transport::OutboundTx;

/// The board's ⋯ menu.
#[component]
pub fn BoardMenu() -> impl IntoView {
    let store = store::use_store();
    let tx = use_context::<OutboundTx>();
    let entries = Signal::derive(move || store.with(|s| entries(store, s, tx.as_ref())));
    view! {
        <div class="board-menu">
            <MoreMenu entries=entries/>
        </div>
    }
}

/// The menu's entries for `state`, each member's sending through `tx`, then
/// Delete for the creator, which a finished game offers too.
fn entries(store: StoreSignal, state: &ClientState, tx: Option<&OutboundTx>) -> Vec<MenuEntry> {
    let delete = state
        .game_id
        .clone()
        .filter(|_| state.you.is_some() && state.you == state.creator)
        .map(|game_id| delete_game::delete_entry(store, game_id));
    state
        .reassignments()
        .into_iter()
        .map(|(investigator, members)| {
            let to: Vec<MenuEntry> = members
                .into_iter()
                .map(|member| {
                    let to = member.account;
                    let tx = tx.cloned();
                    MenuEntry {
                        label: member.display_name.clone(),
                        on_select: Callback::new(move |()| {
                            if let Some(tx) = &tx {
                                let _ =
                                    tx.unbounded_send(ClientMessage::Reassign { investigator, to });
                            }
                        }),
                        children: Vec::new(),
                    }
                })
                .collect();
            MenuEntry::submenu(
                format!("Reassign {}", state.investigator_name(investigator)),
                to,
            )
        })
        .chain(delete)
        .collect()
}
