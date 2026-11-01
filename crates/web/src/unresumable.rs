//! The unresumable-game overlay (wasm-only).
//!
//! [`ConnStatus::Unresumable`](crate::store::ConnStatus::Unresumable) is
//! terminal: the server closed the socket saying this build can't rebuild the
//! game, and the transport has stopped retrying, since every attempt would
//! fail the same way. Without the overlay the game would just never load, and
//! the player wouldn't learn why.
//!
//! Modelled on the version-mismatch overlay: a scrim and a card over the
//! board, with no click handler on the scrim. The saved game id is kept, so a
//! rollback to the previous build can still resume the game. The button moves
//! on without it.

use leptos::prelude::*;

use crate::store::{self, ConnStatus};
use crate::transport;

/// The terminal unresumable-game overlay. Renders only at
/// [`ConnStatus::Unresumable`].
#[component]
pub fn UnresumableView() -> impl IntoView {
    let store = store::use_store();

    view! {
        {move || {
            if store.with(|s| s.status != ConnStatus::Unresumable) {
                return ().into_any();
            }
            view! {
                <div class="ur-backdrop"></div>
                <section class="unresumable">
                    <p class="ur-detail">
                        "This game can't be resumed on this server version"
                    </p>
                    <button
                        class="ur-new-game"
                        on:click=move |_| transport::start_new_game()
                    >
                        "New game"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}
