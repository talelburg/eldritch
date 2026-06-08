//! The root Leptos component for the Eldritch web client.

use leptos::prelude::*;

use crate::store::{provide_store, use_store, ConnStatus};

#[component]
pub fn App() -> impl IntoView {
    provide_store();

    // Spawn the browser transport only on wasm; native/headless-reducer
    // builds render from a signal that tests drive directly.
    #[cfg(target_arch = "wasm32")]
    {
        let store = use_store();
        crate::transport::start(store);
    }

    view! {
        <main>
            <h1>"Eldritch"</h1>
            <DebugDump/>
        </main>
    }
}

/// Read-only dump of the client store — proves the round-trip before
/// P6.5's real board. Renders a stable presence label (for the headless
/// test) plus a human-facing pretty-print of the state.
#[component]
pub fn DebugDump() -> impl IntoView {
    let store = use_store();

    let status = move || match store.get().status {
        ConnStatus::Connecting => "connecting",
        ConnStatus::Connected => "connected",
        ConnStatus::Reconnecting => "reconnecting",
        ConnStatus::Failed => "failed",
    };
    let presence = move || {
        if store.get().game.is_some() {
            "present"
        } else {
            "none"
        }
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();
    let dump = move || {
        store
            .get()
            .game
            .map_or_else(|| "<no state yet>".to_string(), |g| format!("{g:#?}"))
    };

    view! {
        <section>
            <p>"status: " {status}</p>
            <p>"state: " {presence}</p>
            <p>"rejection: " {rejection}</p>
            <pre>{dump}</pre>
        </section>
    }
}
