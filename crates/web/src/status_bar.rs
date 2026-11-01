//! The app status bar, rendered inline with the page heading.
//!
//! It shows the connection status, the last rejection, who the game is
//! waiting on, the New-game button, and the signed-in account with its
//! sign-out control. It reads the store; the buttons act only on wasm, where
//! the transport is.

use std::collections::BTreeSet;

use game_core::engine::{EngineOutcome, Responder};
use game_core::state::{GameState, InvestigatorId};
use leptos::prelude::*;

use crate::store::{self, ClientState, ConnStatus};
#[cfg(target_arch = "wasm32")]
use crate::transport;

/// A horizontal status strip (status · rejection · New game) for the app header.
#[component]
pub fn StatusBarView() -> impl IntoView {
    let store = store::use_store();

    let status = move || match store.get().status {
        ConnStatus::Connecting => "connecting",
        ConnStatus::Connected => "connected",
        ConnStatus::Reconnecting => "reconnecting",
        ConnStatus::Restarting => "server restarting",
        ConnStatus::Failed => "failed",
        ConnStatus::AwaitingRoster => "awaiting-roster",
        ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
        ConnStatus::SignedOut(_) => "signed out",
        ConnStatus::Unresumable => "unresumable game",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();
    let waiting = move || {
        store.with(waiting_on).map(|names| {
            view! { <span class="waiting-on">"Waiting on: " {names}</span> }
        })
    };

    let account = move || {
        store.get().account.map(|account| {
            view! {
                <span class="account">"signed in as " {account.display_name}</span>
                <button class="sign-out" on:click=move |_| sign_out()>
                    "Sign out"
                </button>
            }
        })
    };

    view! {
        <div class="status-bar">
            <span class="status">"status: " {status}</span>
            <span class="rejection">"rejection: " {rejection}</span>
            {waiting}
            {
                #[cfg(target_arch = "wasm32")]
                {
                    view! {
                        <button
                            class="new-game"
                            on:click=move |_| transport::start_new_game()
                        >
                            "New game"
                        </button>
                    }
                    .into_any()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ().into_any()
                }
            }
            {account}
        </div>
    }
}

/// Sign out through the transport. Off wasm there is no transport, and the
/// control does nothing.
fn sign_out() {
    #[cfg(target_arch = "wasm32")]
    transport::sign_out();
}

/// Who the live prompt waits on, by investigator name, read off its
/// [`Responder`]: one investigator, the lead (marked as such), or each
/// investigator a window several answer offers an option to. `None` with no
/// prompt live, or no game to name anyone from.
fn waiting_on(state: &ClientState) -> Option<String> {
    let Some(EngineOutcome::AwaitingInput { request, .. }) = &state.outcome else {
        return None;
    };
    let game = state.game.as_ref()?;
    match request.responder {
        Responder::Investigator(id) => Some(name(game, id)),
        Responder::Lead(id) => Some(format!("{} (lead)", name(game, id))),
        Responder::Several => {
            let offered: BTreeSet<InvestigatorId> = request
                .options
                .iter()
                .filter_map(|option| option.investigator)
                .collect();
            (!offered.is_empty()).then(|| {
                offered
                    .into_iter()
                    .map(|id| name(game, id))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        }
        // `Responder` is non-exhaustive; a kind this client does not know
        // names nobody it can show.
        _ => None,
    }
}

/// An investigator's name, or their seat's id if the game does not hold them.
fn name(game: &GameState, id: InvestigatorId) -> String {
    game.investigators
        .get(&id)
        .map_or_else(|| format!("investigator {}", id.0), |inv| inv.name.clone())
}
