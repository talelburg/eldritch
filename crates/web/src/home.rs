//! The home page (wasm-only): what a page whose address names no game shows.
//!
//! Its New game button creates a lobby and goes to it.

use leptos::prelude::*;

use crate::store::{self, ConnStatus};
use crate::transport;

/// The home page. Renders only at [`ConnStatus::Home`].
#[component]
pub fn HomeView() -> impl IntoView {
    let store = store::use_store();
    let shown = Memo::new(move |_| store.with(|s| s.status == ConnStatus::Home));

    move || {
        shown.get().then(|| {
            view! {
                <section class="home">
                    <button class="home-new-game" on:click=move |_| transport::new_game(store)>
                        "New game"
                    </button>
                </section>
            }
        })
    }
}
