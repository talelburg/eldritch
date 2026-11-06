//! The app status bar, rendered inline with the page heading.
//!
//! It shows the connection status, the last rejection, the New-game button,
//! and the signed-in account with its sign-out control. Whom the game waits
//! on is shown on the board instead, by
//! [`prompt_layout`](crate::prompt_layout). It reads the store; the buttons
//! act only on wasm, where the transport is.

use leptos::prelude::*;

use crate::store::{self, ConnStatus};
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
        ConnStatus::Home => "no game",
        ConnStatus::InLobby => "in lobby",
        ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
        ConnStatus::SignedOut(_) => "signed out",
        ConnStatus::Unresumable => "unresumable game",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();
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
            {
                #[cfg(target_arch = "wasm32")]
                {
                    view! {
                        <button
                            class="new-game"
                            on:click=move |_| transport::new_game(store)
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
