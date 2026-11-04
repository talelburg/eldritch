//! The toast (wasm-only): a short-lived note of why a command this client
//! sent was refused, such as a Take another member beat it to.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::store;

/// How long a toast shows before it dismisses itself.
const SHOWN_FOR_MILLIS: u32 = 4_000;

/// The store's [`toast`](crate::store::ClientState::toast), while it has one.
///
/// It sits outside the lockout wrapper, above the lobby and below the
/// terminal overlays, and clears itself after [`SHOWN_FOR_MILLIS`]. The timer
/// lives here rather than in the store, so the store's reduce stays
/// deterministic. A timer clears only the toast it was started for, so one
/// replaced by a different refusal stays for its own full time.
#[component]
pub fn Toast() -> impl IntoView {
    let store = store::use_store();
    let text = Memo::new(move |_| store.with(|s| s.toast.clone()));
    Effect::new(move |_| {
        let Some(shown) = text.get() else {
            return;
        };
        spawn_local(async move {
            TimeoutFuture::new(SHOWN_FOR_MILLIS).await;
            // `try_update`: the page may have unmounted the store meanwhile.
            store.try_update(|s| {
                if s.toast.as_ref() == Some(&shown) {
                    s.toast = None;
                }
            });
        });
    });

    move || {
        text.get()
            .map(|text| view! { <div class="toast" role="status">{text}</div> })
    }
}
