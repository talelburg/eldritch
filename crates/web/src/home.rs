//! The "Your games" page (wasm-only): what a page whose address names no game
//! shows.
//!
//! It lists the games the signed-in account is a member of, from the
//! listing the transport fetched into the store, in the groups [`Group`]
//! orders, and its heading carries the New game button, which creates a lobby
//! and goes to it.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use game_core::card_registry::CardRegistry;
use game_core::state::Phase;
use leptos::prelude::*;
use protocol::live::LiveSummary;
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
    /// Started games waiting on an investigator the account holds.
    NeedsYou,
    /// Games still in their lobby.
    Lobby,
    /// Started games waiting on someone else, or on nobody.
    InProgress,
}

impl Group {
    /// The group an entry is listed under.
    fn of(entry: &GameEntry) -> Self {
        match &entry.state {
            GameEntryState::Lobby => Group::Lobby,
            GameEntryState::Live(summary) if summary.needs_you() => Group::NeedsYou,
            GameEntryState::Live(_) => Group::InProgress,
        }
    }

    /// The group's heading.
    fn title(self) -> &'static str {
        match self {
            Group::NeedsYou => "Needs you",
            Group::Lobby => "Lobby",
            Group::InProgress => "In progress",
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
            {pills(cards, &entry)}
            <span class="row-status">{status_line(&entry)}</span>
            <span class="row-activity">{local_time(entry.last_activity)}</span>
        </a>
    }
}

/// A pill for each waited-on investigator the account holds, naming it:
/// "Daisy Walker needs you". None for a game that doesn't need the account.
fn pills(cards: &'static CardRegistry, entry: &GameEntry) -> impl IntoView {
    let GameEntryState::Live(summary) = &entry.state else {
        return Vec::new();
    };
    summary
        .yours()
        .map(|waited| {
            let text = format!("{} needs you", names::card_name(cards, &waited.card));
            view! { <span class="row-pill">{text}</span> }
        })
        .collect()
}

/// What a row says of where its game stands.
fn status_line(entry: &GameEntry) -> String {
    match &entry.state {
        GameEntryState::Lobby => format!(
            "Waiting for {} to start",
            entry.name(entry.creator).unwrap_or("its creator")
        ),
        GameEntryState::Live(summary) => live_status(entry, summary),
    }
}

/// "Round {n} · {Phase} · waiting for {accounts}", the accounts holding the
/// investigators the live prompt waits on, joined with "and". The last part
/// is left off when nobody is waited on.
fn live_status(entry: &GameEntry, summary: &LiveSummary) -> String {
    let place = format!("Round {} · {}", summary.round, phase_name(summary.phase));
    let waiting_for: Vec<&str> = summary
        .waiting_for()
        .into_iter()
        .map(|account| entry.name(account).unwrap_or("Unknown"))
        .collect();
    if waiting_for.is_empty() {
        place
    } else {
        format!("{place} · waiting for {}", joined_with_and(&waiting_for))
    }
}

/// `names` joined with commas, the last two with "and": "Sam", "Sam and
/// Tal", "Alex, Sam and Tal".
fn joined_with_and(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The phase's name.
fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Mythos => "Mythos",
        Phase::Investigation => "Investigation",
        Phase::Enemy => "Enemy",
        Phase::Upkeep => "Upkeep",
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
