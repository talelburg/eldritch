//! The board's ⋯ menu: Reassign `<investigator>`, over the members each may go
//! to.
//!
//! Its entries are [`ClientState::reassignments`]: the creator's cover every
//! investigator, a holder's only their own, a viewer has none, and a finished
//! game offers none. Picking a member sends the reassignment; the board
//! changes only once the `Assigned` it causes arrives. wasm-only — it sends
//! through the wasm-only `OutboundTx`.

use leptos::prelude::*;
use protocol::ClientMessage;

use crate::more_menu::{MenuEntry, MoreMenu};
use crate::store::{self, ClientState};
use crate::transport::OutboundTx;

/// The board's ⋯ menu.
#[component]
pub fn BoardMenu() -> impl IntoView {
    let store = store::use_store();
    let tx = use_context::<OutboundTx>();
    let entries = Signal::derive(move || store.with(|s| entries(s, tx.as_ref())));
    view! {
        <div class="board-menu">
            <MoreMenu entries=entries/>
        </div>
    }
}

/// The menu's entries for `state`, each member's sending through `tx`.
fn entries(state: &ClientState, tx: Option<&OutboundTx>) -> Vec<MenuEntry> {
    state
        .reassignments()
        .into_iter()
        .map(|(investigator, members)| {
            let to: Vec<MenuEntry> = members
                .into_iter()
                .map(|member| {
                    let to = member.account;
                    let tx = tx.cloned();
                    MenuEntry::new(member.display_name.clone(), move || {
                        if let Some(tx) = &tx {
                            let _ = tx.unbounded_send(ClientMessage::Reassign { investigator, to });
                        }
                    })
                })
                .collect();
            MenuEntry::submenu(
                format!("Reassign {}", state.investigator_name(investigator)),
                to,
            )
        })
        .collect()
}
