//! Core-loop action controls (P6.7a, wasm-only). Buttons that submit the
//! toy scenario's actions, each `disabled` per the P6.6 legality helper
//! ([`enabled_controls`](crate::legality::enabled_controls)) — a UX
//! affordance, not a correctness gate (the server stays authoritative).
//! Move/PlayCard use inline pickers; Mulligan has its own multi-select.
//! `board.rs` stays read-only — all interactivity lives here.

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

/// All core-loop action controls. Reads the store reactively; nothing
/// renders until both a `game` and an `outcome` are present.
#[component]
pub fn ActionControls() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();

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
                </section>
            }
            .into_any()
        }}
    }
}
