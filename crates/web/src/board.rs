//! Render of `GameState` into the DOM.
//!
//! Panels are plain helper fns, and `BoardView` is this module's only
//! component; the interactive pieces (hand and in-play cards, the anchored
//! controls) are components from their own modules. Cards and locations render
//! as their names, read from the registry provided at mount
//! ([`registry::use_cards`]).

use game_core::engine::{self, OptionTarget};
use game_core::scenario::ScenarioEnding;
use game_core::state::investigator::{Investigator, InvestigatorId};
use game_core::state::GameState;
use leptos::prelude::*;

use crate::card::{HandCardView, InPlayCardView};
use crate::controls::{AnchoredControl, PlayerDeckView};
use crate::enemy_card::EnemyCard;
use crate::prompt_layout::{self, OpenTabSignal, Placement, PromptLayout};
use crate::interaction::{self, PendingOptions};
use crate::{act_agenda, map, registry, store};

/// Read-only board. Always renders a status line (connection status +
/// last rejection); renders the map and the dock when a game is present, else
/// a placeholder.
#[component]
pub fn BoardView() -> impl IntoView {
    let store = store::use_store();

    let board = move || {
        let state = store.get();
        let placed = prompt_layout::layout(&state, prompt_layout::use_open_tab());
        match state.game {
            None => view! { <p class="no-game">"<no game>"</p> }.into_any(),
            Some(game) => view! {
                <div class="game">
                    {board_menu()}
                    {resolution_banner(&game)}
                    {act_agenda::act_agenda_view(&game)}
                    <div class="board-main">
                        // The map pans by scrolling inside this box, never by
                        // scaling: a `transform` would misplace the fixed
                        // context menu and popover inside it.
                        <div class="map-box">{map::location_map(&game)}</div>
                        {dock(&game, &placed)}
                    </div>
                </div>
            }
            .into_any(),
        }
    };

    view! {
        <section class="board">
            {board}
        </section>
    }
}

/// `investigator`'s prompt strip, on wasm, where it can send.
fn prompt_strip(investigator: InvestigatorId) -> AnyView {
    #[cfg(target_arch = "wasm32")]
    {
        view! { <crate::prompt_strip::PromptStrip investigator=investigator/> }.into_any()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = investigator;
        ().into_any()
    }
}

/// The board's ⋯ menu, on wasm, where it can send.
fn board_menu() -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    {
        view! { <crate::board_menu::BoardMenu/> }.into_any()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ().into_any()
    }
}

/// The tabs and the dock: one tab per investigator in seating order, carrying
/// its marks (tinted when this account holds them, ▶ for whose turn it is, ★
/// for the lead, a spinner while another account answers for them, "needs
/// you" while the prompt asks one of yours whose tab isn't open, and the
/// card's glow while an option is anchored to their investigator card), and
/// below them the open tab's investigator. A tab click opens it.
fn dock(game: &GameState, placed: &PromptLayout) -> impl IntoView {
    let open_tab_signal = use_context::<OpenTabSignal>();
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let tabs: Vec<_> = placed
        .tabs
        .iter()
        .map(|tab| {
            let investigator = tab.investigator;
            let name = game
                .investigators
                .get(&investigator)
                .map(|inv| inv.name.clone())
                .unwrap_or_default();
            let spinner = (tab.asked == Some(Placement::Spinner))
                .then(|| view! { <span class="spinner" role="img" aria-label="deciding"></span> });
            let carries_option = game
                .investigators
                .get(&investigator)
                .is_some_and(|inv| !interaction::options_for(&pending, &inv.card_anchor()).is_empty());
            let needs_you = placed.needs_you(investigator);
            let needs_you_mark =
                needs_you.then(|| view! { <span class="tab-needs-you">"needs you"</span> });
            view! {
                <button
                    class="inv-tab"
                    class:held=tab.held
                    class:needs-you=needs_you
                    class:actionable=carries_option
                    class:open=placed.open_investigator == Some(investigator)
                    on:click=move |_| {
                        if let Some(signal) = open_tab_signal {
                            signal.click(investigator);
                        }
                    }
                >
                    {tab.active.then(|| view! { <span class="tab-active">"▶"</span> })}
                    <span class="tab-name">{name}</span>
                    {tab.lead.then(|| view! { <span class="tab-lead">"★"</span> })}
                    {spinner}
                    {needs_you_mark}
                </button>
            }
        })
        .collect();
    let docked = placed.open_investigator.and_then(|open| {
        let held = placed.holds(open);
        let strip = placed.dock_strip == Some(open);
        game.investigators
            .get(&open)
            .map(|inv| investigator_panel(game, inv, held, strip))
    });
    view! {
        <section class="dock">
            <nav class="inv-tabs">{tabs}</nav>
            {docked}
        </section>
    }
}

/// One investigator's panel in the dock: name, their prompt strip when it is
/// docked, stats, actions, resources, clues and status; in play, the threat
/// area with engaged enemies, discard, deck and hand.
fn investigator_panel(
    game: &GameState,
    inv: &Investigator,
    held: bool,
    strip: bool,
) -> impl IntoView {
    let strip = strip.then(|| prompt_strip(inv.id));
    let cards = registry::use_cards();
    let inv_id = inv.id;
    let deck_remaining = inv.deck.len();
    let hand = hand_view(inv, held);
    let in_play: Vec<_> = inv
        .cards_in_play
        .iter()
        .cloned()
        .map(|c| view! { <InPlayCardView instance=c/> })
        .collect();
    let threat: Vec<_> = inv
        .threat_area
        .iter()
        .cloned()
        .map(|c| view! { <InPlayCardView instance=c/> })
        .collect();
    let engaged: Vec<_> = game
        .enemies
        .values()
        .filter(|e| e.engaged_with == Some(inv.id))
        .cloned()
        .map(|e| view! { <EnemyCard enemy=e/> })
        .collect();
    let vitals = view! {
        <div class="inv-vitals">
            <span class="inv-skills">
                "W" {inv.skills.willpower} " I" {inv.skills.intellect}
                " C" {inv.skills.combat} " A" {inv.skills.agility}
            </span>
            <span class="inv-hp">"hp " {inv.damage()} "/" {inv.max_health(cards)}</span>
            <span class="inv-san">"san " {inv.horror()} "/" {inv.max_sanity(cards)}</span>
        </div>
    };
    // Standard plus additional: "Skids" O'Toole's bought action is a pip too.
    let pips: Vec<_> = (0..engine::actions_available(game, cards, inv_id))
        .map(|_| view! { <span class="pip">"●"</span> })
        .collect();
    view! {
        <article class="investigator">
            <h3 class="inv-name">{inv.name.clone()}</h3>
            {strip}
            <div class="inv-zones-top">
                <div class="in-play"><h4>"In play"</h4><div class="card-row">{in_play}</div></div>
                <div class="threat"><h4>"Threat area"</h4><div class="card-row">{threat}{engaged}</div></div>
            </div>
            <div class="inv-zones-bottom">
                <div class="investigator-block">
                    <div class="investigator-card">
                        <InPlayCardView instance=inv.investigator_card.clone()/>
                        {vitals}
                    </div>
                    <div class="inv-meta">
                        <span class="inv-actions">"actions " {pips}</span>
                        <span class="inv-resources">
                            "resources " {inv.resources}
                            // The Gain-resource affordance sits with the
                            // number it changes.
                            <AnchoredControl
                                label="Gain resource".to_string()
                                class="resource-control"
                                target=OptionTarget::ResourcePool(inv_id)
                            />
                        </span>
                        <span class="inv-clues">"clues " {inv.clues}</span>
                        <span class="inv-status">{format!("{:?}", inv.status)}</span>
                    </div>
                    // End turn lives on the investigator's own panel, in
                    // the same place all game — greyed out off-turn rather
                    // than vanishing.
                    <AnchoredControl
                        label="End turn".to_string()
                        class="turn-control"
                        target=OptionTarget::TurnControl(inv_id)
                    />
                </div>
                <div class="discard">
                    <h4>"Discard"</h4>
                    <span class="discard-count">{inv.discard.len()}</span>
                </div>
                // Each investigator shows their *own* deck, so a
                // multiplayer board never leaves a count ambiguous.
                <PlayerDeckView
                    investigator=inv_id
                    remaining=deck_remaining
                />
                <div class="hand"><h4>"Hand"</h4>{hand}</div>
            </div>
        </article>
    }
}

/// An investigator's hand: their cards when this account `held` them, and
/// otherwise a count of face-down cards, so no card of it reaches the DOM.
fn hand_view(inv: &Investigator, held: bool) -> AnyView {
    if !held {
        return view! {
            <div class="hand-hidden">
                <div class="deck-back"></div>
                <span class="hand-count">{inv.hand.len()}</span>
            </div>
        }
        .into_any();
    }
    let inv_id = inv.id;
    let slots: Vec<_> = inv
        .hand
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, code)| {
            let index = u8::try_from(i).unwrap_or(u8::MAX);
            view! {
                <HandCardView code=code investigator=inv_id index=index/>
            }
        })
        .collect();
    view! { <div class="card-row">{slots}</div> }.into_any()
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
