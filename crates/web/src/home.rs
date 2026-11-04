//! The "Your games" page (wasm-only): what a page whose address names no game
//! shows.
//!
//! It lists the games the signed-in account is a member of, from the
//! listing the transport fetched into the store, in the groups [`Group`]
//! orders, and its heading carries the New game button, which creates a lobby
//! and goes to it.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use leptos::prelude::*;
use protocol::{AccountId, GameEntry, GameEntryState};
use wasm_bindgen::JsValue;

use crate::store::{self, ConnStatus};
use crate::{names, registry, transport, url};

/// The "Your games" page. Renders only at [`ConnStatus::Home`].
#[component]
pub fn HomeView() -> impl IntoView {
    let store = store::use_store();
    let shown = Memo::new(move |_| store.with(|s| s.status == ConnStatus::Home));
    let games = Memo::new(move |_| store.with(|s| s.games.clone().unwrap_or_default()));

    move || {
        shown.get().then(|| {
            view! {
                <section class="home">
                    <header class="games-heading">
                        <h1>"Your games"</h1>
                        <button class="home-new-game" on:click=move |_| transport::new_game(store)>
                            "New game"
                        </button>
                    </header>
                    {move || {
                        grouped(&games.get())
                            .into_iter()
                            .map(|(group, entries)| view! { <GameGroup group entries/> })
                            .collect_view()
                    }}
                </section>
            }
        })
    }
}

/// A group of the page, in the order the page shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// Games still in their lobby.
    Lobby,
}

impl Group {
    /// The group an entry is listed under.
    fn of(entry: &GameEntry) -> Self {
        match entry.state {
            GameEntryState::Lobby => Group::Lobby,
        }
    }

    /// The group's heading.
    fn title(self) -> &'static str {
        match self {
            Group::Lobby => "Lobby",
        }
    }
}

/// `entries` in their groups, in group order, each group's newest first. A
/// group with no entry is absent.
fn grouped(entries: &[GameEntry]) -> Vec<(Group, Vec<GameEntry>)> {
    let mut groups: BTreeMap<Group, Vec<GameEntry>> = BTreeMap::new();
    for entry in entries {
        groups
            .entry(Group::of(entry))
            .or_default()
            .push(entry.clone());
    }
    groups
        .into_iter()
        .map(|(group, mut entries)| {
            entries.sort_by_key(|entry| Reverse(entry.last_activity));
            (group, entries)
        })
        .collect()
}

/// One group: its heading, then its rows.
#[component]
fn GameGroup(group: Group, entries: Vec<GameEntry>) -> impl IntoView {
    view! {
        <section class="games-group">
            <h2 class="games-group-title">{group.title()}</h2>
            <ul class="game-rows">
                {entries.into_iter().map(|entry| view! { <li><GameRow entry/></li> }).collect_view()}
            </ul>
        </section>
    }
}

/// One game's row, which opens the game's link.
#[component]
fn GameRow(entry: GameEntry) -> impl IntoView {
    let cards = registry::use_cards();
    let name = |account: AccountId| entry.name(account).unwrap_or("Unknown").to_owned();
    let investigators = entry
        .investigators
        .iter()
        .map(|seat| {
            let investigator = names::card_name(cards, &seat.investigator);
            let holder = name(seat.holder);
            view! {
                <li class="row-investigator">
                    {investigator}
                    {seat.lead.then(|| view! { <span class="lead-star">"★"</span> })}
                    <span class="row-holder">{holder}</span>
                </li>
            }
        })
        .collect_view();
    view! {
        <a class="game-row" href=url::game_path(&entry.game_id)>
            <span class="row-scenario">{names::scenario_name(&entry.scenario_id).to_owned()}</span>
            <span class="row-creator">{format!("Created by {}", name(entry.creator))}</span>
            <ul class="row-investigators">{investigators}</ul>
            <span class="row-status">{status_line(&entry)}</span>
            <span class="row-activity">{local_time(entry.last_activity)}</span>
        </a>
    }
}

/// What a row says of where its game stands.
fn status_line(entry: &GameEntry) -> String {
    match entry.state {
        GameEntryState::Lobby => format!(
            "Waiting for {} to start",
            entry.name(entry.creator).unwrap_or("its creator")
        ),
    }
}

/// `millis` since the Unix epoch, as a date and time in the browser's locale
/// and time zone.
fn local_time(millis: i64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "millis since the epoch stay well inside f64's exact integers"
    )]
    let date = js_sys::Date::new(&JsValue::from_f64(millis as f64));
    date.to_locale_string("default", &JsValue::UNDEFINED).into()
}
