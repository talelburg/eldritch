//! Read-only render of `GameState` into the DOM (P6.5). Panels are plain
//! helper fns; `BoardView` is the only component. Cards and locations render as
//! their names via `crate::names` (the client installs `cards::REGISTRY`).

use game_core::state::GameState;
use game_core::ScenarioEnding;
use leptos::prelude::*;

use crate::store::use_store;

/// Read-only board. Always renders a status line (connection status +
/// last rejection); renders the panels when a game is present, else a
/// placeholder.
#[component]
pub fn BoardView() -> impl IntoView {
    let store = use_store();

    let board = move || match store.get().game {
        None => view! { <p class="no-game">"<no game>"</p> }.into_any(),
        Some(game) => view! {
            <div class="game">
                {resolution_banner(&game)}
                {crate::act_agenda::act_agenda_view(&game)}
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
            let inv_id = inv.id;
            let deck_remaining = inv.deck.len();
            let hand: Vec<_> = inv
                .hand
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, code)| {
                    let index = u8::try_from(i).unwrap_or(u8::MAX);
                    view! {
                        <crate::card::HandCardView code=code investigator=inv_id index=index/>
                    }
                })
                .collect();
            let in_play: Vec<_> = inv
                .cards_in_play
                .iter()
                .cloned()
                .map(|c| view! { <crate::card::InPlayCardView instance=c/> })
                .collect();
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .cloned()
                .map(|c| view! { <crate::card::InPlayCardView instance=c/> })
                .collect();
            let engaged: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.engaged_with == Some(inv.id))
                .cloned()
                .map(|e| view! { <crate::enemy_card::EnemyCard enemy=e/> })
                .collect();
            let vitals = view! {
                <div class="inv-vitals">
                    <span class="inv-skills">
                        "W" {inv.skills.willpower} " I" {inv.skills.intellect}
                        " C" {inv.skills.combat} " A" {inv.skills.agility}
                    </span>
                    <span class="inv-hp">"hp " {inv.damage()} "/" {inv.max_health()}</span>
                    <span class="inv-san">"san " {inv.horror()} "/" {inv.max_sanity()}</span>
                </div>
            };
            let pips: Vec<_> = (0..inv.actions_remaining)
                .map(|_| view! { <span class="pip">"●"</span> })
                .collect();
            view! {
                <article class="investigator">
                    <h3 class="inv-name">{inv.name.clone()}</h3>
                    <div class="inv-zones-top">
                        <div class="in-play"><h4>"In play"</h4><div class="card-row">{in_play}</div></div>
                        <div class="threat"><h4>"Threat area"</h4><div class="card-row">{threat}{engaged}</div></div>
                    </div>
                    <div class="inv-zones-bottom">
                        <div class="investigator-block">
                            <div class="investigator-card">
                                <crate::card::InPlayCardView instance=inv.investigator_card.clone()/>
                                {vitals}
                            </div>
                            <div class="inv-meta">
                                <span class="inv-actions">"actions " {pips}</span>
                                <span class="inv-resources">
                                    "resources " {inv.resources}
                                    // The Gain-resource affordance sits with the
                                    // number it changes (#541).
                                    <crate::controls::AnchoredControl
                                        label="Gain resource".to_string()
                                        class="resource-control"
                                        target=game_core::OptionTarget::ResourcePool(inv_id)
                                    />
                                </span>
                                <span class="inv-clues">"clues " {inv.clues}</span>
                                <span class="inv-status">{format!("{:?}", inv.status)}</span>
                            </div>
                            // End turn lives on the investigator's own panel, in
                            // the same place all game — greyed out off-turn rather
                            // than vanishing (#541).
                            <crate::controls::AnchoredControl
                                label="End turn".to_string()
                                class="turn-control"
                                target=game_core::OptionTarget::TurnControl(inv_id)
                            />
                        </div>
                        // Each investigator shows their *own* deck, so a
                        // multiplayer board never leaves a count ambiguous.
                        <crate::controls::PlayerDeckView
                            investigator=inv_id
                            remaining=deck_remaining
                        />
                        <div class="hand"><h4>"Hand"</h4><div class="card-row">{hand}</div></div>
                    </div>
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

/// Scenario-ending banner — rendered only once `GameState.ending` latches.
/// Read-only display of state, matching the pure-fn display pattern; keeps
/// `board.rs` read-only (no new interactivity).
///
/// It names the ending rather than declaring a win or a loss. A scenario ends
/// at a resolution point or at none, and that is what the player carries to the
/// campaign guide's "do not read until end of game" section. Win and loss are a
/// standalone-mode projection — *"they win if they complete a resolution on an
/// act card"* — and which resolutions are favorable is scenario-local knowledge
/// the campaign guide owns, not something this pure function can compute. See
/// `docs/adr/0012-a-scenario-ends-at-a-resolution-point-or-at-none.md`.
fn resolution_banner(game: &GameState) -> impl IntoView {
    game.ending.map(|ending| {
        let text = match ending {
            // `ResolutionId` renders as the campaign guide titles it,
            // e.g. "Resolution 3".
            ScenarioEnding::Resolution(id) => format!("Scenario ended — {id}"),
            ScenarioEnding::NoResolution => "Scenario ended — no resolution reached".to_string(),
            // `ScenarioEnding` is #[non_exhaustive]; a future variant gets a
            // generic banner until the client learns its shape.
            _ => "Scenario ended".to_string(),
        };
        view! { <section class="resolution">{text}</section> }
    })
}
