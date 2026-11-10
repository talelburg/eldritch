//! The event log: a read-only, accumulating view of the game's events, newest
//! at the bottom, grouped per submitted action, in a drawer that starts closed.

use leptos::html::Div;
use leptos::prelude::*;

use crate::{card, store};

/// Read-only event log in a drawer. Closed, it is only its toggle, so the board
/// keeps the whole screen; open, it lies over the board's left side. Renders
/// every accumulated `LogBatch` oldest-first (newest at the bottom); a header
/// line per batch then one Debug line per event, each as this account may
/// read it ([`ClientState::log_header`](store::ClientState::log_header),
/// [`ClientState::log_line`](store::ClientState::log_line)). The body stays rendered while
/// closed, only hidden, so opening the drawer shows the log as it already is.
/// On wasm, auto-scrolls to the bottom as the log grows.
#[component]
pub fn EventLogView() -> impl IntoView {
    let store = store::use_store();
    let open = RwSignal::new(false);
    let scroll_ref = NodeRef::<Div>::new();

    // Keep the panel pinned to the newest line. Re-runs whenever a batch is
    // appended; the scroll is deferred to the next animation frame so it reads
    // `scroll_height` *after* the new rows are laid out (reading it inline races
    // the reactive DOM update and lands short of the true bottom).
    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            let _ = store.with(|s| s.log.len());
            let _ = open.get();
            request_animation_frame(move || {
                // Untracked: a DOM handle, read outside any reactive scope, that
                // nothing should react to.
                if let Some(el) = scroll_ref.get_untracked() {
                    el.set_scroll_top(el.scroll_height());
                }
            });
        });
    }

    // Every line is rendered for the account as it stands: a reassignment
    // re-renders the log, so a hidden card follows its investigator's holder.
    let batches = move || {
        store.with(|s| {
            s.log
                .iter()
                .map(|batch| {
                    let events: Vec<_> = batch
                        .events
                        .iter()
                        .map(|e| view! { <div class="log-event">{s.log_line(e)}</div> })
                        .collect();
                    let header = s.log_header(&batch.header);
                    view! {
                        <div class="log-batch">
                            <div class="log-action">"▸ "{card::render_label(&header)}</div>
                            {events}
                        </div>
                    }
                })
                .collect::<Vec<_>>()
        })
    };

    view! {
        <aside class="event-log" class:open=move || open.get()>
            <div class="event-log-head">
                <h2>"Event log"</h2>
                <button
                    class="log-toggle"
                    on:click=move |_| open.update(|o| *o = !*o)
                >
                    {move || if open.get() { "◂ hide" } else { "▸ log" }}
                </button>
            </div>
            <div
                class="log-scroll"
                class:hidden=move || !open.get()
                node_ref=scroll_ref
            >
                {batches}
            </div>
        </aside>
    }
}
