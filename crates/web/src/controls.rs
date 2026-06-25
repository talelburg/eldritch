//! Core-loop action controls (P6.7a, wasm-only). After 2b (#447) the only
//! bespoke control is `StartScenario`: the engine surfaces open-turn gameplay as
//! an `AwaitingInput` action menu, rendered by `AwaitingInputView` (input.rs),
//! which submits `ResolveInput(PickSingle(OptionId))`. The setup mulligan and
//! every other suspension flow through that same prompt UI. `board.rs` stays
//! read-only — interactivity lives here (session start) and in the prompt view.

use game_core::PlayerAction;
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::legality::{enabled_controls, ActionControl};
use crate::store::use_store;
use crate::transport::OutboundTx;

/// A single zero-payload action button. `class` carries a test-stable hook
/// (e.g. `"action start-scenario"`); `disabled` reflects legality; the click
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

/// The remaining bespoke control: `StartScenario` (the one pre-game action that
/// precedes any `AwaitingInput`). Every in-game action flows through
/// `AwaitingInputView` — the engine's
/// open-turn action menu and framework prompts, submitted as `ResolveInput`.
/// Reads the store reactively; nothing renders until both a `game` and an
/// `outcome` are present.
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

            view! {
                <section class="controls">
                    {submit_button(
                        "action start-scenario",
                        "Start scenario",
                        !enabled.contains(&ActionControl::StartScenario),
                        tx.clone(),
                        PlayerAction::StartScenario { roster: vec![] },
                    )}
                    // The open-turn action menu and every framework suspension
                    // (mulligan, encounter draw, commit/reaction windows, …)
                    // render through `AwaitingInputView` (input.rs) as the engine
                    // offers them — no dedicated control per action (#447).
                </section>
            }
            .into_any()
        }}
    }
}
