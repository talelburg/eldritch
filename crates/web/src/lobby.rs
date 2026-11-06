//! The lobby (wasm-only): what a member sees of a game before Start.
//!
//! The header holds the members, the invite link with Copy and the ⋯ menu;
//! below it the banner, on the opening that made this account a member; then
//! the pool of five investigators, the seating's four numbered positions below
//! the pool, and Start at the end of the seating.
//!
//! Each part is a component of its own, reading what it shows from the store's
//! [`LobbyView`](protocol::lobby::LobbyView).

use cards::decks::{self, DefaultDeck, WeaknessChoice};
use game_core::state::CardCode;
use leptos::prelude::*;
use protocol::lobby::{Direction, MAX_PICKS};
use protocol::ClientMessage;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::more_menu::{MenuEntry, MoreMenu};
use crate::store::{self, ClientState, ConnStatus, StoreSignal};
use crate::transport::OutboundTx;
use crate::{delete_game, names, registry, url};

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
/// with a button that copies it, and the ⋯ menu.
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
    let entries = Signal::derive(move || store.with(|s| menu_entries(store, s)));

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
            <MoreMenu entries=entries/>
        </header>
    }
}

/// The header's ⋯ entries for the store's signed-in account: Remove
/// `<member>` for each other member then Delete if it created the lobby, and
/// Leave if it didn't. The creator stays, so the lobby always has someone to
/// order the seating and press Start.
fn menu_entries(store: StoreSignal, s: &ClientState) -> Vec<MenuEntry> {
    let (Some(lobby), Some(you)) = (&s.lobby, s.you) else {
        return Vec::new();
    };
    if !s.is_creator(Some(lobby.creator)) {
        return vec![MenuEntry::action("Leave", || send(ClientMessage::Leave))];
    }
    let removes = lobby
        .members
        .iter()
        .filter(|member| member.account != you)
        .map(|member| {
            let account = member.account;
            MenuEntry::action(format!("Remove {}", member.display_name), move || {
                send(ClientMessage::Remove { member: account });
            })
        });
    removes
        .chain(
            s.game_id
                .clone()
                .map(|game_id| delete_game::delete_entry(store, game_id)),
        )
        .collect()
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
            lobby.member_name(lobby.creator).map(str::to_string)
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
    /// Nobody: free to take, unless [`MAX_PICKS`] investigators are picked.
    Free {
        /// [`MAX_PICKS`] investigators are already picked.
        full: bool,
    },
    /// This account, with the pick's weakness choice.
    Yours(WeaknessChoice),
    /// Another member, by display name, with the pick's weakness choice.
    Held(String, WeaknessChoice),
}

/// Who holds `investigator` in the store's lobby, as the store's signed-in
/// account sees it.
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
        return Holding::Yours(pick.weakness);
    }
    Holding::Held(
        lobby
            .member_name(pick.account)
            .unwrap_or_default()
            .to_string(),
        pick.weakness,
    )
}

/// Send `message` to the server, if a transport is mounted.
fn send(message: ClientMessage) {
    if let Some(tx) = use_context::<OutboundTx>() {
        let _ = tx.unbounded_send(message);
    }
}

/// One investigator of the pool, named, and marked with who holds them: free
/// (Take), yours (the weakness choice, and Give up), or another member's
/// (their name, and the default deck the pick plays with its weakness choice,
/// read-only).
#[component]
fn PoolEntry(deck: &'static DefaultDeck) -> impl IntoView {
    let store = store::use_store();
    let holding = Memo::new(move |_| store.with(|s| holding(s, deck.investigator)));
    let investigator = move || CardCode::new(deck.investigator);
    let name = names::card_name(registry::use_cards(), &investigator());

    view! {
        <div class="pool-entry" data-investigator=deck.investigator>
            <span class="pool-name">{name}</span>
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
                Holding::Yours(weakness) => view! {
                    <span class="pool-state">"Yours"</span>
                    <label class="pool-weakness">
                        <input
                            type="checkbox"
                            class="random-basic-weakness"
                            prop:checked=weakness == WeaknessChoice::Random
                            on:change=move |_| {
                                let choice = match weakness {
                                    WeaknessChoice::Fixed => WeaknessChoice::Random,
                                    WeaknessChoice::Random => WeaknessChoice::Fixed,
                                };
                                send(ClientMessage::SetWeakness { investigator: investigator(), choice });
                            }
                        />
                        " Random basic weakness"
                    </label>
                    <button
                        class="give-up"
                        on:click=move |_| send(ClientMessage::GiveUp { investigator: investigator() })
                    >
                        "Give up"
                    </button>
                }
                .into_any(),
                Holding::Held(member, weakness) => view! {
                    <span class="pool-state">{member}</span>
                    <DeckList deck=deck weakness=weakness/>
                }
                .into_any(),
            }}
        </div>
    }
}

/// `deck`'s cards, read-only: the counted cards, then the signatures, a
/// repeated card shown once with its count, then the basic weakness `weakness`
/// chooses: the fixed one, or a random one drawn at Start.
#[component]
fn DeckList(deck: &'static DefaultDeck, weakness: WeaknessChoice) -> impl IntoView {
    let cards = registry::use_cards();
    let card_name = |code: &str| names::card_name(cards, &CardCode::new(code));
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for code in deck.cards.iter().chain(deck.signatures) {
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
            <li class="pool-weakness">
                {match weakness {
                    WeaknessChoice::Fixed => card_name(deck.weakness),
                    WeaknessChoice::Random => "Random basic weakness".to_string(),
                }}
            </li>
        </ul>
    }
}

/// The seating, below the pool: [`MAX_PICKS`] numbered positions in clockwise
/// order, the picks filling them from the first, and Start at its end.
#[component]
fn Seating() -> impl IntoView {
    view! {
        <ol class="seating">
            {(0..MAX_PICKS).map(|position| view! { <SeatingPosition position=position/> }).collect_view()}
        </ol>
        <StartFooter/>
    }
}

/// What the end of the seating shows this account.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Footer {
    /// The creator's Start, which seats this many picks.
    Start {
        /// How many investigators are picked.
        picks: usize,
    },
    /// Every other member's line, naming the creator.
    Waiting {
        /// The creator's display name.
        creator: String,
    },
}

/// The end of the seating: Start for the creator, saying what pressing it
/// commits to, and unavailable until there is a pick. Every other member sees
/// whom the lobby waits on in its place.
#[component]
fn StartFooter() -> impl IntoView {
    let store = store::use_store();
    let footer = Memo::new(move |_| {
        store.with(|s| {
            let lobby = s.lobby.as_ref()?;
            Some(if s.you == Some(lobby.creator) {
                Footer::Start {
                    picks: lobby.picks.len(),
                }
            } else {
                Footer::Waiting {
                    creator: lobby
                        .member_name(lobby.creator)
                        .unwrap_or_default()
                        .to_string(),
                }
            })
        })
    });

    move || {
        footer.get().map(|footer| match footer {
            Footer::Start { picks } => view! {
                <button
                    class="start"
                    disabled=picks == 0
                    on:click=move |_| send(ClientMessage::Start)
                >
                    {start_label(picks)}
                </button>
            }
            .into_any(),
            Footer::Waiting { creator } => view! {
                <p class="waiting">{format!("Waiting for {creator} to start")}</p>
            }
            .into_any(),
        })
    }
}

/// What the creator's Start says it commits to, with `picks` investigators
/// picked.
fn start_label(picks: usize) -> String {
    let investigators = if picks == 1 {
        "investigator"
    } else {
        "investigators"
    };
    format!("Starts with {picks} {investigators}. The roster can't change after.")
}

/// One position of the seating, from 0: the pick sitting there, named with
/// its member, and the ★ if it leads. Empty while nobody sits there. The
/// creator also gets the [`SeatingControls`].
#[component]
fn SeatingPosition(position: usize) -> impl IntoView {
    let store = store::use_store();
    let cards = registry::use_cards();
    let seated = Memo::new(move |_| {
        store.with(|s| {
            let lobby = s.lobby.as_ref()?;
            let pick = lobby.picks.get(position)?;
            let text = format!(
                "{} ({})",
                names::card_name(cards, &pick.investigator),
                lobby.member_name(pick.account).unwrap_or_default()
            );
            Some((text, lobby.lead.as_ref() == Some(&pick.investigator)))
        })
    });
    let leads = move || seated.with(|seated| seated.as_ref().is_some_and(|(_, lead)| *lead));

    view! {
        <li class="seating-position" class:lead=leads>
            <span class="position-number">{position + 1}</span>
            <span class="position-pick">
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
                    send(ClientMessage::MoveInSeating {
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
