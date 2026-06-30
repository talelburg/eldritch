//! The app status bar: connection status, last rejection, and the New-game
//! button — rendered inline with the page heading to keep the header compact.
//! Reads the store; the New-game button is wasm-only (it drives the transport).

use leptos::prelude::*;

use crate::store::{use_store, ConnStatus};

/// A horizontal status strip (status · rejection · New game) for the app header.
#[component]
pub fn StatusBarView() -> impl IntoView {
    let store = use_store();

    let status = move || match store.get().status {
        ConnStatus::Connecting => "connecting",
        ConnStatus::Connected => "connected",
        ConnStatus::Reconnecting => "reconnecting",
        ConnStatus::Failed => "failed",
        ConnStatus::AwaitingRoster => "awaiting-roster",
        ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();

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
                            on:click=move |_| crate::transport::start_new_game()
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
        </div>
    }
}
