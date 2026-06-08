//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards render as their
//! `CardCode` strings — the client has no card-name source.

use game_core::state::{GameState, LocationId};
use leptos::prelude::*;

use crate::store::{use_store, ConnStatus};

/// Read-only board. Always renders a status line (connection status +
/// last rejection); renders the panels when a game is present, else a
/// placeholder.
#[component]
pub fn BoardView() -> impl IntoView {
    let store = use_store();

    let status = move || match store.get().status {
        ConnStatus::Connecting => "connecting",
        ConnStatus::Connected => "connected",
        ConnStatus::Reconnecting => "reconnecting",
        ConnStatus::Failed => "failed",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();

    let board = move || match store.get().game {
        None => view! { <p class="no-game">"<no game>"</p> }.into_any(),
        Some(game) => view! {
            <div class="game">
                {phase_bar(&game)}
                {locations_panel(&game)}
                {investigators_panel(&game)}
            </div>
        }
        .into_any(),
    };

    view! {
        <section class="board">
            <p class="status">"status: " {status}</p>
            <p class="rejection">"rejection: " {rejection}</p>
            {board}
        </section>
    }
}

/// One row per location: name, shroud, clues, and a revealed flag.
/// Iterates the `BTreeMap` in `LocationId` order (deterministic).
fn locations_panel(game: &GameState) -> impl IntoView {
    let rows: Vec<_> = game
        .locations
        .values()
        .map(|loc| {
            let revealed = if loc.revealed {
                "revealed"
            } else {
                "unrevealed"
            };
            view! {
                <li class="location">
                    <span class="loc-name">{loc.name.clone()}</span>
                    <span class="loc-shroud">"shroud " {loc.shroud}</span>
                    <span class="loc-clues">"clues " {loc.clues}</span>
                    <span class="loc-revealed">{revealed}</span>
                </li>
            }
        })
        .collect();
    view! {
        <section class="locations">
            <h2>"Locations"</h2>
            <ul>{rows}</ul>
        </section>
    }
}

/// One panel per investigator: name, location, actions, resources,
/// health (`damage/max_health`), sanity (`horror/max_sanity`), clues,
/// status; hand and cards-in-play as text lists of card codes.
fn investigators_panel(game: &GameState) -> impl IntoView {
    let panels: Vec<_> = game
        .investigators
        .values()
        .map(|inv| {
            let location = inv
                .current_location
                .map_or_else(|| "—".to_string(), |LocationId(id)| format!("loc {id}"));
            let hand: Vec<_> = inv
                .hand
                .iter()
                .map(|code| view! { <li class="card">{code.to_string()}</li> })
                .collect();
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .map(|c| view! { <li class="card">{c.code.to_string()}</li> })
                .collect();
            view! {
                <article class="investigator">
                    <h3 class="inv-name">{inv.name.clone()}</h3>
                    <span class="inv-location">{location}</span>
                    <span class="inv-actions">"actions " {inv.actions_remaining}</span>
                    <span class="inv-resources">"resources " {inv.resources}</span>
                    <span class="inv-health">"health " {inv.damage} "/" {inv.max_health}</span>
                    <span class="inv-sanity">"sanity " {inv.horror} "/" {inv.max_sanity}</span>
                    <span class="inv-clues">"clues " {inv.clues}</span>
                    <span class="inv-status">{format!("{:?}", inv.status)}</span>
                    <div class="hand"><h4>"Hand"</h4><ul>{hand}</ul></div>
                    <div class="in-play"><h4>"In play"</h4><ul>{in_play}</ul></div>
                </article>
            }
        })
        .collect();
    view! {
        <section class="investigators">
            <h2>"Investigators"</h2>
            {panels}
        </section>
    }
}

/// Phase + round, plus the current act's clue threshold and the current
/// agenda's doom (`doom/threshold`). Act/agenda lines are omitted when
/// their decks are empty (fixtures may omit them).
fn phase_bar(game: &GameState) -> impl IntoView {
    let phase = format!("{:?}", game.phase);
    let round = game.round;
    let act = game
        .act_deck
        .get(game.act_index)
        .map(|a| format!("clues 0/{}", a.clue_threshold));
    let agenda = game
        .agenda_deck
        .get(game.agenda_index)
        .map(|a| format!("doom {}/{}", game.agenda_doom, a.doom_threshold));
    view! {
        <header class="phase-bar">
            <span class="phase">{phase}</span>
            <span class="round">"round " {round}</span>
            {agenda.map(|t| view! { <span class="agenda">{t}</span> })}
            {act.map(|t| view! { <span class="act">{t}</span> })}
        </header>
    }
}
