//! The lobby (wasm-only): what a member sees of a game before Start.
//!
//! It is laid out as #1231's variant C. The header holds the members and the
//! invite link with Copy; below it the banner, on the opening that made this
//! account a member; then the pool of five investigators, and the seating's
//! four numbered positions below the pool.
//!
//! Each part is a component of its own, reading what it shows from the store's
//! [`LobbyView`](protocol::lobby::LobbyView).

use cards::decks::{self, DefaultDeck};
use game_core::state::CardCode;
use leptos::prelude::*;
use protocol::lobby::{Direction, LobbyView, MAX_PICKS};
use protocol::{AccountId, ClientMessage};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::store::{self, ClientState, ConnStatus};
use crate::transport::OutboundTx;
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
                    <Seating/>
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

/// Who holds one investigator of the pool, as this account sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Holding {
    /// Nobody: free to take, unless the table is full.
    Free {
        /// [`MAX_PICKS`] investigators are already picked.
        full: bool,
    },
    /// This account.
    Yours,
    /// Another member, by display name.
    Held(String),
}

/// Who holds `investigator` in the store's lobby, as the account it names
/// `you` sees it.
fn holding(s: &ClientState, investigator: &str) -> Holding {
    let Some(lobby) = &s.lobby else {
        return Holding::Free { full: false };
    };
    let Some(pick) = lobby
        .picks
        .iter()
        .find(|pick| pick.investigator.as_str() == investigator)
    else {
        return Holding::Free {
            full: lobby.picks.len() >= MAX_PICKS,
        };
    };
    if s.you == Some(pick.account) {
        return Holding::Yours;
    }
    Holding::Held(display_name(lobby, pick.account))
}

/// The display name of the member `account`, from `lobby`.
fn display_name(lobby: &LobbyView, account: AccountId) -> String {
    lobby
        .members
        .iter()
        .find(|member| member.account == account)
        .map_or_else(String::new, |member| member.display_name.clone())
}

/// The printed name of the card `code`, which the corpus holds.
fn card_name(code: &str) -> String {
    cards::by_code(code)
        .unwrap_or_else(|| panic!("a default deck's card {code} is in the corpus"))
        .name
        .clone()
}

/// Send `message` to the server, if a transport is mounted.
fn send(message: ClientMessage) {
    if let Some(tx) = use_context::<OutboundTx>() {
        let _ = tx.unbounded_send(message);
    }
}

/// One investigator of the pool, named, and marked with who holds them: free
/// (Take), yours (Give up), or another member's (their name, and the default
/// deck the pick plays, read-only).
#[component]
fn PoolEntry(deck: &'static DefaultDeck) -> impl IntoView {
    let store = store::use_store();
    let holding = Memo::new(move |_| store.with(|s| holding(s, deck.investigator)));
    let investigator = move || CardCode::new(deck.investigator);

    view! {
        <div class="pool-entry" data-investigator=deck.investigator>
            <span class="pool-name">{card_name(deck.investigator)}</span>
            {move || match holding.get() {
                Holding::Free { full } => view! {
                    <span class="pool-state">"Free"</span>
                    <button
                        class="take"
                        disabled=full
                        on:click=move |_| send(ClientMessage::Take { investigator: investigator() })
                    >
                        "Take"
                    </button>
                }
                .into_any(),
                Holding::Yours => view! {
                    <span class="pool-state">"Yours"</span>
                    <button
                        class="give-up"
                        on:click=move |_| send(ClientMessage::GiveUp { investigator: investigator() })
                    >
                        "Give up"
                    </button>
                }
                .into_any(),
                Holding::Held(member) => view! {
                    <span class="pool-state">{member}</span>
                    <DeckList deck=deck/>
                }
                .into_any(),
            }}
        </div>
    }
}

/// `deck`'s cards, read-only: the counted cards, then the signatures, then the
/// fixed weakness, a repeated card shown once with its count.
#[component]
fn DeckList(deck: &'static DefaultDeck) -> impl IntoView {
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for code in deck
        .cards
        .iter()
        .chain(deck.signatures)
        .chain([&deck.weakness])
    {
        match counted.iter_mut().find(|(seen, _)| seen == code) {
            Some((_, count)) => *count += 1,
            None => counted.push((code, 1)),
        }
    }
    view! {
        <ul class="pool-deck">
            {counted
                .into_iter()
                .map(|(code, count)| {
                    let name = card_name(code);
                    let line = if count > 1 { format!("{count}× {name}") } else { name };
                    view! { <li>{line}</li> }
                })
                .collect_view()}
        </ul>
    }
}

/// The seating, below the pool: [`MAX_PICKS`] numbered positions in clockwise
/// order, the picks filling them from the first.
#[component]
fn Seating() -> impl IntoView {
    view! {
        <ol class="seating">
            {(0..MAX_PICKS).map(|position| view! { <Seat position=position/> }).collect_view()}
        </ol>
    }
}

/// One position of the seating, from 0: the pick sitting there, named with
/// its member, and the ★ if it leads. Empty while nobody sits there. The
/// creator also gets the [`SeatingControls`].
#[component]
fn Seat(position: usize) -> impl IntoView {
    let store = store::use_store();
    let seated = Memo::new(move |_| {
        store.with(|s| {
            let lobby = s.lobby.as_ref()?;
            let pick = lobby.picks.get(position)?;
            let text = format!(
                "{} ({})",
                card_name(pick.investigator.as_str()),
                display_name(lobby, pick.account)
            );
            Some((text, lobby.lead.as_ref() == Some(&pick.investigator)))
        })
    });
    let leads = move || seated.with(|seated| seated.as_ref().is_some_and(|(_, lead)| *lead));

    view! {
        <li class="seat" class:lead=leads>
            <span class="seat-number">{position + 1}</span>
            <span class="seat-pick">
                {move || seated.get().map(|(text, _)| text).unwrap_or_default()}
            </span>
            {move || leads().then(|| view! { <span class="lead-star">"★"</span> })}
            <SeatingControls position=position/>
        </li>
    }
}

/// What the creator can do with the pick at a position of the seating.
#[derive(Clone, PartialEq)]
struct Controls {
    investigator: CardCode,
    /// Whether the pick sits in position 1, with nobody to swap with ◀.
    first: bool,
    /// Whether the pick sits last, with nobody to swap with ▶.
    last: bool,
    /// Whether the pick already leads, so has no ★ to press.
    leads: bool,
}

/// The creator's controls on the pick at `position`, from 0: ◀ ▶ to move it
/// along the seating, disabled at the ends, and a ★ to make it the lead
/// unless it already leads. Nothing for any other member, or at an empty
/// position.
#[component]
fn SeatingControls(position: usize) -> impl IntoView {
    let store = store::use_store();
    let controls = Memo::new(move |_| {
        store.with(|s| {
            let lobby = s.lobby.as_ref()?;
            if s.you != Some(lobby.creator) {
                return None;
            }
            let pick = lobby.picks.get(position)?;
            Some(Controls {
                investigator: pick.investigator.clone(),
                first: position == 0,
                last: position + 1 == lobby.picks.len(),
                leads: lobby.lead.as_ref() == Some(&pick.investigator),
            })
        })
    });

    move || {
        controls.get().map(|controls| {
            let moved = |direction: Direction| {
                let investigator = controls.investigator.clone();
                move |_| {
                    send(ClientMessage::MoveSeat {
                        investigator: investigator.clone(),
                        direction,
                    });
                }
            };
            let starred = controls.investigator.clone();
            view! {
                <span class="seating-controls">
                    <button
                        class="move-earlier"
                        title="Move earlier in the seating"
                        disabled=controls.first
                        on:click=moved(Direction::Earlier)
                    >
                        "◀"
                    </button>
                    <button
                        class="move-later"
                        title="Move later in the seating"
                        disabled=controls.last
                        on:click=moved(Direction::Later)
                    >
                        "▶"
                    </button>
                    {(!controls.leads)
                        .then(|| {
                            view! {
                                <button
                                    class="set-lead"
                                    title="Make the lead investigator"
                                    on:click=move |_| {
                                        send(ClientMessage::SetLead {
                                            investigator: starred.clone(),
                                        });
                                    }
                                >
                                    "★"
                                </button>
                            }
                        })}
                </span>
            }
        })
    }
}
