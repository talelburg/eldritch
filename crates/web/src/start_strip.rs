//! The start strip (wasm-only): the one-off strip a member present at Start
//! sees above the board, naming the roster Start froze and its ★ lead.

use leptos::prelude::*;

use crate::store::{self, StripEntry};

/// The store's [`start_strip`](crate::store::ClientState::start_strip), while
/// it has one, with a button that dismisses it for good.
///
/// It sits in the board's flow, above the board inside the lockout wrapper,
/// rather than over it: it is shown only once the board is live, and covers
/// nothing.
#[component]
pub fn StartStrip() -> impl IntoView {
    let store = store::use_store();
    let entries = Memo::new(move |_| store.with(|s| s.start_strip.clone()));

    move || {
        entries.get().map(|entries| {
            view! {
                <div class="start-strip" role="status">
                    <span class="strip-title">"Started with"</span>
                    <ul class="strip-roster">
                        {entries
                            .into_iter()
                            .map(|entry| view! { <li class="strip-entry">{label(&entry)}</li> })
                            .collect_view()}
                    </ul>
                    <button class="dismiss" on:click=move |_| store.update(|s| s.start_strip = None)>
                        "Dismiss"
                    </button>
                </div>
            }
        })
    }
}

/// `entry` as the strip names it: the investigator, their member, and the ★ if
/// they lead.
fn label(entry: &StripEntry) -> String {
    let star = if entry.lead { " ★" } else { "" };
    format!("{} ({}){star}", entry.investigator, entry.member)
}
