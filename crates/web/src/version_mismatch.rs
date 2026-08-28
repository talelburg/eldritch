//! The version-mismatch overlay (#770, wasm-only).
//!
//! [`ConnStatus::VersionMismatch`](crate::store::ConnStatus::VersionMismatch) is
//! terminal: the transport sets it on the first un-parseable server frame and
//! stops reconnecting (#463), so nothing on the board behind it will ever
//! respond again. One line in the header did not carry that — the board stayed
//! fully rendered and clickable, which reads exactly like a stalled engine, and
//! a skew filed as an engine bug is what corrupts a playthrough's findings.
//!
//! So the state gets a scrim and a card, above every other overlay. The board
//! stays *visible* beneath it — it is the evidence of what was happening when
//! the skew hit — but obviously inert. The scrim has no click handler: the state
//! is terminal, and there is nothing to dismiss it back to.
//!
//! The button is only half the fix. The usual cause is a stale *server* binary,
//! which a reload cannot restart, so the card names both halves and the button
//! does the second.

use leptos::prelude::*;

use crate::store::{use_store, ConnStatus};

/// The terminal wire-format-skew overlay. Renders only at
/// [`ConnStatus::VersionMismatch`].
#[component]
pub fn VersionMismatchView() -> impl IntoView {
    let store = use_store();

    view! {
        {move || {
            if store.get().status != ConnStatus::VersionMismatch {
                return ().into_any();
            }
            view! {
                <div class="vm-backdrop"></div>
                <section class="version-mismatch">
                    <p class="vm-title">"version mismatch"</p>
                    <p class="vm-detail">
                        "The client and server binaries disagree on the wire \
                         format, so this board is frozen — it is not the engine \
                         waiting on you. Restart the server, then reload."
                    </p>
                    <button
                        class="vm-reload"
                        on:click=move |_| crate::transport::reload()
                    >
                        "Reload"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}
