//! The unresumable-game overlay (wasm-only).
//!
//! [`ConnStatus::Unresumable`](crate::store::ConnStatus::Unresumable) is
//! terminal: the server closed the socket saying this build can't rebuild the
//! game, and the transport has stopped retrying, since every attempt would
//! fail the same way. Without the overlay the game would just never load, and
//! the player wouldn't learn why.
//!
//! Modelled on the version-mismatch overlay: a scrim and a card over the
//! board, with no click handler on the scrim. The game's link still names it,
//! so a rollback to the previous build can resume the game. The button moves
//! on to a new one.
//!
//! When the "Your games" listing the transport fetches has an entry for the
//! game, the card adds where it stopped and why, the build it started on and
//! the build the server runs. A non-member has no entry, and sees the
//! sentence alone.

use leptos::prelude::*;
use protocol::{GameEntryState, UnresumableSummary};

use crate::home;
use crate::store::{self, ClientState, ConnStatus};
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
            let summary = store.with(this_games_summary);
            view! {
                <div class="ur-backdrop"></div>
                <section class="unresumable">
                    <p class="ur-detail">
                        "This game can't be resumed on this server version"
                    </p>
                    {summary.map(|summary| view! { <Detail summary/> })}
                    <button
                        class="ur-new-game"
                        on:click=move |_| transport::new_game(store)
                    >
                        "New game"
                    </button>
                </section>
            }
            .into_any()
        }}
    }
}

/// The listing's summary of the game the page is on, if the listing has it
/// as an unresumable game.
fn this_games_summary(state: &ClientState) -> Option<UnresumableSummary> {
    let game_id = state.game_id.as_ref()?;
    let entry = state
        .games
        .as_ref()?
        .iter()
        .find(|entry| entry.game_id == *game_id)?;
    let GameEntryState::Unresumable(summary) = &entry.state else {
        return None;
    };
    Some(summary.clone())
}

/// Where the game stopped and why, the two builds, and that the game is kept.
#[component]
fn Detail(summary: UnresumableSummary) -> impl IntoView {
    view! {
        <p class="ur-reason">{home::unresumable_status(&summary)}</p>
        <p class="ur-builds">
            {format!(
                "It started on build {}, and the server runs build {}.",
                summary.created_build,
                summary.server_build,
            )}
        </p>
        <p class="ur-kept">
            "The game is kept as it was: if the server rolls back to its build, it opens again."
        </p>
    }
}
