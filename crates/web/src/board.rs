//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards and locations render as
//! their names via `crate::names` (the client installs `cards::REGISTRY`).

use game_core::state::GameState;
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
                <div class="board-main">
                    {crate::map::location_map(&game)}
                    {investigators_panel(&game)}
                </div>
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
                .cloned()
                .map(|code| view! { <crate::card::Card code=code/> })
                .collect();
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .map(|c| view! { <li class="card-line">{crate::names::card_name(&c.code)}</li> })
                .collect();
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .map(|c| view! { <li class="card-line">{crate::names::card_name(&c.code)}</li> })
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
                    <div class="hand"><h4>"Hand"</h4><div class="card-row">{hand}</div></div>
                    <div class="in-play"><h4>"In play"</h4><ul>{in_play}</ul></div>
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}{engaged}</ul></div>
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
