//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards render as their
//! `CardCode` strings — the client has no card-name source.

use game_core::state::GameState;
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
