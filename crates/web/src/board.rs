//! Render of `GameState` into the DOM.
//!
//! Panels are plain helper fns, and `BoardView` and its dock are this module's
//! components; the interactive pieces (hand and in-play tiles, the anchored
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
use crate::interaction::{self, PendingOptions};
use crate::prompt_layout::{self, OpenTabSignal, Placement, PromptLayout};
use crate::{act_agenda, map, registry, store};

/// Read-only board. Always renders a status line (connection status +
/// last rejection); renders the map and the dock when a game is present, else
/// a placeholder.
///
/// Each part re-renders on its own as the store changes, rebuilding its DOM in
/// place, so the map keeps its pan and a card row its scroll across an update.
/// The dock's panel is the exception: it is keyed by the open tab, so each tab
/// opens on a panel of its own.
#[component]
pub fn BoardView() -> impl IntoView {
    let store = store::use_store();
    let has_game = Memo::new(move |_| store.with(|s| s.game.is_some()));

    let board = move || {
        if !has_game.get() {
            return view! { <p class="no-game">"<no game>"</p> }.into_any();
        }
        let header = move || {
            store.with(|s| {
                s.game.as_ref().map(|game| {
                    view! {
                        {board_menu()}
                        {resolution_banner(game)}
                        {act_agenda::act_agenda_view(game)}
                    }
                })
            })
        };
        let map = move || store.with(|s| s.game.as_ref().map(map::location_map));
        view! {
            <div class="game">
                {header}
                <div class="board-main">
                    // The map pans by scrolling inside this box, never by
                    // scaling: a `transform` would misplace the fixed
                    // context menu and popover inside it.
                    <div class="map-box">{map}</div>
                    <Dock/>
                </div>
            </div>
        }
        .into_any()
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
///
/// The panel is keyed by the open investigator: another tab opens on a panel
/// built for it, its rows scrolled to the start, rather than on the last tab's
/// rows refilled.
#[component]
fn Dock() -> impl IntoView {
    let store = store::use_store();
    let open = Memo::new(move |_| {
        let open_tab = prompt_layout::use_open_tab();
        store.with(|s| prompt_layout::layout(s, open_tab).open_investigator)
    });
    let tabs = move || {
        let state = store.get();
        let placed = prompt_layout::layout(&state, prompt_layout::use_open_tab());
        state.game.as_ref().map(|game| tabs_view(game, &placed))
    };
    let panel = move |investigator: InvestigatorId| {
        move || {
            let state = store.get();
            let placed = prompt_layout::layout(&state, prompt_layout::use_open_tab());
            let held = placed.holds(investigator);
            let strip = placed.dock_strip == Some(investigator);
            state.game.as_ref().and_then(|game| {
                game.investigators
                    .get(&investigator)
                    .map(|inv| investigator_panel(game, inv, held, strip))
            })
        }
    };
    view! {
        <section class="dock">
            <nav class="inv-tabs">{tabs}</nav>
            <For each=move || open.get() key=|id| *id children=panel/>
        </section>
    }
}

/// The tabs over the dock, from `placed`'s: see [`Dock`].
fn tabs_view(game: &GameState, placed: &PromptLayout) -> impl IntoView {
    let open_tab_signal = use_context::<OpenTabSignal>();
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    placed
        .tabs
        .iter()
        .map(|tab| {
            let investigator = tab.investigator;
            let inv = game.investigators.get(&investigator);
            let name = inv.map(|inv| inv.name.clone()).unwrap_or_default();
            let carries_option = inv.is_some_and(|inv| {
                !interaction::options_for(&pending, &inv.card_anchor()).is_empty()
            });
            let spinner = (tab.asked == Some(Placement::Spinner))
                .then(|| view! { <span class="spinner" role="img" aria-label="deciding"></span> });
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
        .collect::<Vec<_>>()
}

/// One investigator's panel in the dock, in two rows. Their prompt strip, when
/// it is docked, heads it. The top row is the investigator: their card (which,
/// with the tab, names them), their stats, deck, discard and End turn, then
/// their cards in play, then their threat area with engaged enemies. The bottom
/// row is their hand. Cards are tiles.
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
    // Standard plus additional: "Skids" O'Toole's bought action is a pip too.
    let pips: Vec<_> = (0..engine::actions_available(game, cards, inv_id))
        .map(|_| view! { <span class="pip">"●"</span> })
        .collect();
    view! {
        <article class="investigator">
            {strip}
            <div class="inv-zones-top">
                <div class="investigator-block">
                    <div class="investigator-card">
                        <InPlayCardView
                            instance=inv.investigator_card.clone()
                            skills=inv.skills
                        />
                    </div>
                    <div class="inv-meta">
                        <span class="inv-hp">"hp " {inv.damage()} "/" {inv.max_health(cards)}</span>
                        <span class="inv-san">"san " {inv.horror()} "/" {inv.max_sanity(cards)}</span>
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
                        <span class="inv-actions">"actions " {pips}</span>
                        <span class="inv-status">{format!("{:?}", inv.status)}</span>
                    </div>
                    <div class="inv-piles">
                        // Each investigator shows their *own* deck, so a
                        // multiplayer board never leaves a count ambiguous.
                        <PlayerDeckView
                            investigator=inv_id
                            remaining=deck_remaining
                        />
                        <div class="discard">
                            <span class="zone-label">"Discard"</span>
                            <span class="discard-count">{inv.discard.len()}</span>
                        </div>
                        // End turn lives on the investigator's own panel, in
                        // the same place all game — greyed out off-turn
                        // rather than vanishing.
                        <AnchoredControl
                            label="End turn".to_string()
                            class="turn-control"
                            target=OptionTarget::TurnControl(inv_id)
                        />
                    </div>
                </div>
                <div class="in-play zone">
                    <span class="zone-label">"In play"</span>
                    <div class="card-row">{in_play}</div>
                </div>
                <div class="threat zone">
                    <span class="zone-label">"Threat area"</span>
                    <div class="card-row">{threat}{engaged}</div>
                </div>
            </div>
            <div class="inv-zones-bottom">
                <div class="hand zone"><span class="zone-label">"Hand"</span>{hand}</div>
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
