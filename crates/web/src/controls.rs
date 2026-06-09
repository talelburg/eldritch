//! Core-loop action controls (P6.7a, wasm-only). Buttons that submit the
//! toy scenario's actions, each `disabled` per the P6.6 legality helper
//! ([`enabled_controls`](crate::legality::enabled_controls)) — a UX
//! affordance, not a correctness gate (the server stays authoritative).
//! Move/PlayCard use inline pickers; Mulligan has its own multi-select.
//! `board.rs` stays read-only — all interactivity lives here.

use std::collections::BTreeSet;

use game_core::state::{GameState, InvestigatorId};
use game_core::PlayerAction;
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::legality::{enabled_controls, ActionControl};
use crate::store::use_store;
use crate::transport::OutboundTx;

/// A single zero-payload action button. `class` carries a test-stable hook
/// (e.g. `"action end-turn"`); `disabled` reflects legality; the click
/// submits `action` when an `OutboundTx` is present (absent in
/// render-only contexts → no-op, matching `AwaitingInputView`).
fn submit_button(
    class: &'static str,
    label: &'static str,
    disabled: bool,
    tx: Option<OutboundTx>,
    action: PlayerAction,
) -> impl IntoView {
    view! {
        <button
            class=class
            disabled=disabled
            on:click=move |_| {
                if let Some(tx) = tx.clone() {
                    let _ = tx.unbounded_send(ClientMessage::Submit { action: action.clone() });
                }
            }
        >
            {label}
        </button>
    }
}

/// Move picker: one button per connected destination (from the active
/// investigator's location's `connections`), labeled by destination name.
/// Empty when `legal` is false or there is no active investigator/location.
fn move_picker(
    game: &GameState,
    active: Option<InvestigatorId>,
    legal: bool,
    tx: Option<&OutboundTx>,
) -> impl IntoView {
    let dests: Vec<_> = if legal {
        active
            .and_then(|inv| game.investigators.get(&inv))
            .and_then(|inv| inv.current_location)
            .and_then(|loc_id| game.locations.get(&loc_id))
            .map(|loc| loc.connections.clone())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|dest_id| {
                let inv = active?;
                let name = game
                    .locations
                    .get(&dest_id)
                    .map_or_else(|| format!("loc {}", dest_id.0), |l| l.name.clone());
                let tx = tx.cloned();
                Some(view! {
                    <button
                        class="move-dest"
                        on:click=move |_| {
                            if let Some(tx) = tx.clone() {
                                let _ = tx.unbounded_send(ClientMessage::Submit {
                                    action: PlayerAction::Move {
                                        investigator: inv,
                                        destination: dest_id,
                                    },
                                });
                            }
                        }
                    >
                        "Move to " {name}
                    </button>
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    view! { <div class="move-picker">{dests}</div> }
}

/// `PlayCard` picker: a "Play" button per card in the active
/// investigator's hand (`hand_index` = position). Empty when `legal` is
/// false or there is no active investigator.
fn play_picker(
    game: &GameState,
    active: Option<InvestigatorId>,
    legal: bool,
    tx: Option<&OutboundTx>,
) -> impl IntoView {
    let buttons: Vec<_> = if legal {
        active
            .and_then(|inv| game.investigators.get(&inv))
            .map(|inv_state| {
                inv_state
                    .hand
                    .iter()
                    .enumerate()
                    .map(|(idx, code)| {
                        let hand_index = u8::try_from(idx).expect("hand fits in u8");
                        let inv = active.expect("active present in this branch");
                        let label = code.to_string();
                        let tx = tx.cloned();
                        view! {
                            <li>
                                <button
                                    class="play-card"
                                    on:click=move |_| {
                                        if let Some(tx) = tx.clone() {
                                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                                action: PlayerAction::PlayCard {
                                                    investigator: inv,
                                                    hand_index,
                                                },
                                            });
                                        }
                                    }
                                >
                                    "Play " {label}
                                </button>
                            </li>
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    view! { <ul class="play-picker">{buttons}</ul> }
}

/// Mulligan multi-select: setup-only (gated on the `mulligan_pending`
/// cursor via the legality helper). Toggling a card flips its index in
/// `selected`; submitting sends the selected indices (empty = legal "keep
/// my hand"). Kept separate from the P6.6 commit window — the shapes
/// diverge (see the design spec). The cursor's investigator owns the
/// redraw, not necessarily the active one.
fn mulligan_picker(
    game: &GameState,
    legal: bool,
    selected: RwSignal<BTreeSet<u32>>,
    tx: Option<&OutboundTx>,
) -> impl IntoView {
    if !legal {
        return ().into_any();
    }
    let cursor = game.mulligan_pending;
    let hand: Vec<String> = cursor
        .and_then(|id| game.investigators.get(&id))
        .map(|inv| inv.hand.iter().map(ToString::to_string).collect())
        .unwrap_or_default();
    let cards: Vec<_> = hand
        .into_iter()
        .enumerate()
        .map(|(idx, code)| {
            let i = u32::try_from(idx).expect("hand fits in u32");
            view! {
                <li>
                    <button
                        class="mull-card"
                        class:selected=move || selected.get().contains(&i)
                        on:click=move |_| selected.update(|s| {
                            if !s.remove(&i) {
                                s.insert(i);
                            }
                        })
                    >
                        {code}
                    </button>
                </li>
            }
        })
        .collect();
    let tx = tx.cloned();
    let on_submit = move |_| {
        if let Some(inv) = cursor {
            let indices: Vec<u8> = selected
                .get()
                .into_iter()
                .map(|i| u8::try_from(i).expect("hand fits in u8"))
                .collect();
            if let Some(tx) = tx.clone() {
                let _ = tx.unbounded_send(ClientMessage::Submit {
                    action: PlayerAction::Mulligan {
                        investigator: inv,
                        indices_to_redraw: indices,
                    },
                });
            }
        }
        selected.set(BTreeSet::new());
    };
    view! {
        <section class="mulligan">
            <ul>{cards}</ul>
            <button class="mulligan-submit" on:click=on_submit>"Mulligan"</button>
        </section>
    }
    .into_any()
}

/// All core-loop action controls. Reads the store reactively; nothing
/// renders until both a `game` and an `outcome` are present.
#[component]
pub fn ActionControls() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();
    // Mulligan's own selection signal — component-lived so it survives the
    // reactive re-render and is cleared on submit.
    let mulligan_sel = RwSignal::new(BTreeSet::<u32>::new());

    view! {
        {move || {
            let state = store.get();
            let (Some(game), Some(outcome)) = (state.game.clone(), state.outcome.clone()) else {
                return ().into_any();
            };
            let enabled = enabled_controls(&game, &outcome);
            let has = |c: ActionControl| enabled.contains(&c);
            let active = game.active_investigator;

            // Investigator-bearing buttons only render with an active
            // investigator; the toy scenario always has one in Investigation.
            let investigate = active.map(|inv| {
                submit_button(
                    "action investigate",
                    "Investigate",
                    !has(ActionControl::Investigate),
                    tx.clone(),
                    PlayerAction::Investigate { investigator: inv },
                )
            });
            let advance_act = active.map(|inv| {
                submit_button(
                    "action advance-act",
                    "Advance act",
                    !has(ActionControl::AdvanceAct),
                    tx.clone(),
                    PlayerAction::AdvanceAct { investigator: inv },
                )
            });

            view! {
                <section class="controls">
                    {investigate}
                    {advance_act}
                    {submit_button(
                        "action end-turn",
                        "End turn",
                        !has(ActionControl::EndTurn),
                        tx.clone(),
                        PlayerAction::EndTurn,
                    )}
                    {submit_button(
                        "action draw-encounter",
                        "Draw encounter",
                        !has(ActionControl::DrawEncounter),
                        tx.clone(),
                        PlayerAction::DrawEncounterCard,
                    )}
                    {move_picker(&game, active, has(ActionControl::Move), tx.as_ref())}
                    {play_picker(&game, active, has(ActionControl::PlayCard), tx.as_ref())}
                    {mulligan_picker(
                        &game,
                        has(ActionControl::Mulligan),
                        mulligan_sel,
                        tx.as_ref(),
                    )}
                </section>
            }
            .into_any()
        }}
    }
}
