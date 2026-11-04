//! The lobby (wasm-only): what a member sees of a game before Start.
//!
//! It is laid out as #1231's variant C. The header holds the members and the
//! invite link with Copy; below it the banner, on the opening that made this
//! account a member; then the pool of five investigators.
//!
//! Each part is a component of its own, reading what it shows from the store's
//! [`LobbyView`](protocol::lobby::LobbyView).

use cards::decks::{self, DefaultDeck};
use leptos::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::store::{self, ConnStatus};
use crate::url;

/// The lobby. Renders only at [`ConnStatus::InLobby`], with a lobby in the
/// store.
#[component]
pub fn LobbyScreen() -> impl IntoView {
    let store = store::use_store();
    let shown =
        Memo::new(move |_| store.with(|s| s.status == ConnStatus::InLobby && s.lobby.is_some()));

    move || {
        shown.get().then(|| {
            view! {
                <section class="lobby">
                    <LobbyHeader/>
                    <JoinedBanner/>
                    <Pool/>
                </section>
            }
        })
    }
}

/// The header row: the members by display name, then the game's invite link
/// with a button that copies it.
#[component]
fn LobbyHeader() -> impl IntoView {
    let store = store::use_store();
    let members = move || {
        store.with(|s| {
            s.lobby
                .iter()
                .flat_map(|lobby| lobby.members.iter())
                .map(|member| member.display_name.clone())
                .collect::<Vec<_>>()
        })
    };
    let link = Memo::new(move |_| {
        store.with(|s| {
            s.game_id
                .as_ref()
                .map(url::current_game_link)
                .unwrap_or_default()
        })
    });
    let copied = RwSignal::new(false);

    view! {
        <header class="lobby-header">
            <ul class="lobby-members">
                {move || {
                    members()
                        .into_iter()
                        .map(|name| view! { <li class="lobby-member">{name}</li> })
                        .collect_view()
                }}
            </ul>
            <div class="invite">
                <span class="invite-link">{move || link.get()}</span>
                <button class="copy-link" on:click=move |_| copy(&link.get_untracked(), copied)>
                    {move || if copied.get() { "Copied" } else { "Copy" }}
                </button>
            </div>
        </header>
    }
}

/// Put `link` on the clipboard, and mark `copied` once it is there.
fn copy(link: &str, copied: RwSignal<bool>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let written = window.navigator().clipboard().write_text(link);
    spawn_local(async move {
        if JsFuture::from(written).await.is_ok() {
            copied.set(true);
        }
    });
}

/// The banner saying whose game opening the link joined. Renders only on the
/// opening that made this account a member: there is no Join button.
#[component]
fn JoinedBanner() -> impl IntoView {
    let store = store::use_store();
    let creator = move || {
        store.with(|s| {
            let lobby = s.lobby.as_ref().filter(|_| s.joined)?;
            lobby
                .members
                .iter()
                .find(|member| member.account == lobby.creator)
                .map(|member| member.display_name.clone())
        })
    };

    move || {
        creator().map(|creator| {
            view! {
                <p class="joined-banner">
                    "You've joined " {creator} "'s game: you're a member of it now. "
                    "Add an investigator, or just watch as a viewer."
                </p>
            }
        })
    }
}

/// The pool: every default deck's investigator, side by side, in
/// [`DEFAULT_DECKS`](decks::DEFAULT_DECKS) order.
#[component]
fn Pool() -> impl IntoView {
    view! {
        <div class="lobby-pool">
            {decks::DEFAULT_DECKS
                .iter()
                .map(|deck| view! { <PoolEntry deck=deck/> })
                .collect_view()}
        </div>
    }
}

/// One investigator of the pool, named, and marked with who holds them.
#[component]
fn PoolEntry(deck: &'static DefaultDeck) -> impl IntoView {
    let name = cards::by_code(deck.investigator)
        .expect("a default deck's investigator is in the corpus")
        .name
        .clone();
    view! {
        <div class="pool-entry" data-investigator=deck.investigator>
            <span class="pool-name">{name}</span>
            <span class="pool-state">"Free"</span>
        </div>
    }
}
