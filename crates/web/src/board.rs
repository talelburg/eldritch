//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards and locations render as
//! their names via `crate::names` (the client installs `cards::REGISTRY`).

use game_core::state::{GameState, InvestigatorId};
use game_core::Resolution;
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
        ConnStatus::AwaitingRoster => "awaiting-roster",
        ConnStatus::VersionMismatch => "version mismatch — restart the server and reload",
    };
    let rejection = move || store.get().last_rejection.unwrap_or_default();

    let board = move || match store.get().game {
        None => view! { <p class="no-game">"<no game>"</p> }.into_any(),
        Some(game) => view! {
            <div class="game">
                {resolution_banner(&game)}
                {phase_bar(&game)}
                {crate::map::location_map(&game)}
                {investigators_panel(&game)}
                {enemies_panel(&game)}
            </div>
        }
        .into_any(),
    };

    view! {
        <section class="board">
            <p class="status">"status: " {status}</p>
            <p class="rejection">"rejection: " {rejection}</p>
            {
                #[cfg(target_arch = "wasm32")]
                {
                    view! {
                        <button
                            class="new-game"
                            on:click=move |_| crate::transport::start_new_game()
                        >
                            "New game"
                        </button>
                    }
                    .into_any()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ().into_any()
                }
            }
            {board}
        </section>
    }
}

/// One row per location: name, shroud, clues, and a revealed flag.
/// Iterates the `BTreeMap` in `LocationId` order (deterministic).
#[allow(dead_code)]
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
/// status; hand and cards-in-play as text lists of card names.
fn investigators_panel(game: &GameState) -> impl IntoView {
    let panels: Vec<_> = game
        .investigators
        .values()
        .map(|inv| {
            let location = inv.current_location.map_or_else(
                || "—".to_string(),
                |id| crate::names::location_name(game, id),
            );
            let hand: Vec<_> = inv
                .hand
                .iter()
                .map(|code| view! { <li class="card">{crate::names::card_name(code)}</li> })
                .collect();
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .map(|c| view! { <li class="card">{crate::names::card_name(&c.code)}</li> })
                .collect();
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .map(|c| view! { <li class="card">{crate::names::card_name(&c.code)}</li> })
                .collect();
            let engaged: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.engaged_with == Some(inv.id))
                .map(|e| {
                    view! {
                        <li class="enemy-engaged">
                            {e.name.clone()} " " {e.damage} "/" {e.max_health}
                        </li>
                    }
                })
                .collect();
            view! {
                <article class="investigator">
                    <h3 class="inv-name">{inv.name.clone()}</h3>
                    <span class="inv-location">{location}</span>
                    <span class="inv-actions">"actions " {inv.actions_remaining}</span>
                    <span class="inv-resources">"resources " {inv.resources}</span>
                    <span class="inv-health">"health " {inv.damage()} "/" {inv.max_health()}</span>
                    <span class="inv-sanity">"sanity " {inv.horror()} "/" {inv.max_sanity()}</span>
                    <span class="inv-clues">"clues " {inv.clues}</span>
                    <span class="inv-status">{format!("{:?}", inv.status)}</span>
                    <div class="hand"><h4>"Hand"</h4><ul>{hand}</ul></div>
                    <div class="in-play"><h4>"In play"</h4><ul>{in_play}</ul></div>
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}</ul></div>
                    <div class="engaged"><h4>"Engaged enemies"</h4><ul>{engaged}</ul></div>
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

/// One row per enemy: name, fight/evade, health (`damage/max_health`),
/// location, and engagement.
fn enemies_panel(game: &GameState) -> impl IntoView {
    let rows: Vec<_> = game
        .enemies
        .values()
        .map(|e| {
            let engaged = match e.engaged_with {
                Some(InvestigatorId(id)) => format!("engaged with {id}"),
                None => "unengaged".to_string(),
            };
            let location = e.current_location.map_or_else(
                || "—".to_string(),
                |id| crate::names::location_name(game, id),
            );
            view! {
                <li class="enemy">
                    <span class="enemy-name">{e.name.clone()}</span>
                    <span class="enemy-fight">"fight " {e.fight}</span>
                    <span class="enemy-evade">"evade " {e.evade}</span>
                    <span class="enemy-health">"health " {e.damage} "/" {e.max_health}</span>
                    <span class="enemy-location">{location}</span>
                    <span class="enemy-engaged">{engaged}</span>
                </li>
            }
        })
        .collect();
    view! {
        <section class="enemies">
            <h2>"Enemies"</h2>
            <ul>{rows}</ul>
        </section>
    }
}

/// Win/loss banner — rendered only once `GameState.resolution` latches.
/// Read-only display of state, matching the `phase_bar` pattern; keeps
/// `board.rs` read-only (no new interactivity).
fn resolution_banner(game: &GameState) -> impl IntoView {
    game.resolution.as_ref().map(|r| {
        let text = match r {
            Resolution::Won { id } => format!("Scenario won — {id}"),
            Resolution::Lost { reason } => format!("Scenario lost — {reason}"),
            // `Resolution` is #[non_exhaustive]; a future variant gets a
            // generic banner until the client learns its shape.
            _ => "Scenario resolved".to_string(),
        };
        view! { <section class="resolution">{text}</section> }
    })
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
