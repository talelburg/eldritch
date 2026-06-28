//! Event-log panel (#505): a read-only, accumulating view of the game's events,
//! left of the board, newest at the bottom, grouped per submitted action.

use crate::store::use_store;
use leptos::prelude::*;

/// Read-only event log, left of the board. Renders every accumulated `LogBatch`
/// oldest-first (newest at the bottom); a header line per batch then one Debug
/// line per event. On wasm, auto-scrolls to the bottom as the log grows.
#[component]
pub fn EventLogView() -> impl IntoView {
    let store = use_store();
    let scroll_ref = NodeRef::<leptos::html::Div>::new();

    // Auto-scroll to the newest line whenever the batch count changes.
    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            let _ = store.with(|s| s.log.len());
            if let Some(el) = scroll_ref.get() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    }

    let batches = move || {
        store
            .get()
            .log
            .into_iter()
            .map(|batch| {
                let events: Vec<_> = batch
                    .events
                    .iter()
                    .map(|e| view! { <div class="log-event">{format!("{e:?}")}</div> })
                    .collect();
                view! {
                    <div class="log-batch">
                        <div class="log-action">{format!("▸ {}", batch.header)}</div>
                        {events}
                    </div>
                }
            })
            .collect::<Vec<_>>()
    };

    view! {
        <aside class="event-log">
            <h2>"Event log"</h2>
            <div class="log-scroll" node_ref=scroll_ref>
                {batches}
            </div>
        </aside>
    }
}
