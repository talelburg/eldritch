//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards render as their
//! `CardCode` strings — the client has no card-name source.

use leptos::prelude::*;

use crate::store::{use_store, ConnStatus};

/// Read-only board. Always renders a status line (connection status +
/// last rejection); renders the panels when a game is present, else a
/// placeholder.
#[component]
pub fn BoardView() -> impl IntoView {
    let store = use_store();

    let status = move || match store.get().status {
        ConnStatus::Connecting => "connecting",
        ConnStatus::Connected => "connected",
        ConnStatus::Reconnecting => "reconnecting",
        ConnStatus::Failed => "failed",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();

    let board = move || match store.get().game {
        None => view! { <p class="no-game">"<no game>"</p> }.into_any(),
        Some(_game) => view! { <p class="has-game">"game present"</p> }.into_any(),
    };

    view! {
        <section class="board">
            <p class="status">"status: " {status}</p>
            <p class="rejection">"rejection: " {rejection}</p>
            {board}
        </section>
    }
}
