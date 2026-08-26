//! The board surfaces the engine names by role (S6, #541): the three
//! always-present investigator-panel controls (End turn, Gain resource, Draw),
//! the per-investigator player deck, and the scenario's encounter deck.
//!
//! Two rendering rules, both from ADR 0011:
//!
//! - **A surface named by role submits directly.** These controls are buttons
//!   that submit on click rather than opening the one-item context menu a board
//!   *entity* opens — a control labelled `"End turn"` carries no ambiguity about
//!   which of several actions it means, so the extra click defends against a
//!   confusion that cannot occur.
//! - **They are always rendered, and `disabled` when nothing anchors to them.**
//!   The panel's shape then does not jump as prompts arrive and resolve, which is
//!   most of what made the retired `.action-bar` worth deleting.
//!
//! Everything here renders on the host build too; only the click handlers are
//! wasm-gated (they submit through the wasm-only `OutboundTx`).

use game_core::state::{GameState, InvestigatorId};
use game_core::{ChoiceOption, OptionTarget};
use leptos::prelude::*;

/// The first option the live prompt anchors to `target`, else `None`. The three
/// named controls carry at most one option each, so "the option" is well-defined.
fn live_option(target: OptionTarget) -> Option<ChoiceOption> {
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    crate::interaction::options_for(&pending, target)
        .into_iter()
        .next()
}

/// Submit `ResolveInput(PickSingle(id))` for a directly-submitting control,
/// recording `label` as the next batch's event-log header. wasm-only — the
/// `OutboundTx` is.
#[cfg(target_arch = "wasm32")]
fn submit_pick(id: game_core::OptionId, label: String) {
    use game_core::{InputResponse, PlayerAction};
    use protocol::ClientMessage;

    let store = crate::store::use_store();
    if let Some(tx) = use_context::<crate::transport::OutboundTx>() {
        store.update(|s| s.pending_label = Some(label));
        let _ = tx.unbounded_send(ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(id),
            },
        });
    }
}

/// Submit `ResolveInput(Confirm)` — the encounter deck's Draw. wasm-only.
#[cfg(target_arch = "wasm32")]
fn submit_confirm(label: String) {
    use game_core::{InputResponse, PlayerAction};
    use protocol::ClientMessage;

    let store = crate::store::use_store();
    if let Some(tx) = use_context::<crate::transport::OutboundTx>() {
        store.update(|s| s.pending_label = Some(label));
        let _ = tx.unbounded_send(ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            },
        });
    }
}

/// A named control anchored to one board surface: always rendered, `disabled`
/// and un-glowing unless the live prompt anchors an option to `target`, and
/// submitting that option directly on click.
///
/// `class` is the control's stable hook (`turn-control`, `resource-control`,
/// `draw-control`) so a test can find it whether or not it is live.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn AnchoredControl(
    /// The button's text — also the event-log header for the submitted batch.
    label: String,
    /// Stable class for this control, independent of its live/dead state.
    class: &'static str,
    /// The surface the engine anchors this control's option to.
    target: OptionTarget,
) -> impl IntoView {
    let option = live_option(target);
    let live = option.is_some();
    #[cfg(target_arch = "wasm32")]
    let on_click = {
        let label = label.clone();
        move |_| {
            if let Some(opt) = option.clone() {
                submit_pick(opt.id, label.clone());
            }
        }
    };
    // The host build renders the same markup inert — `option` is consumed above
    // by `live`, so nothing is left unused.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = option;
    view! {
        <button
            class=class
            class:actionable=live
            disabled=!live
            on:click={
                #[cfg(target_arch = "wasm32")]
                { on_click }
                #[cfg(not(target_arch = "wasm32"))]
                { |_| () }
            }
        >
            {label}
        </button>
    }
}

/// One investigator's own player deck: a card back, the remaining count, and the
/// Draw control anchored to [`OptionTarget::PlayerDeck`].
///
/// Renders at a count of zero without glowing — an investigator whose deck has
/// run out is at their worst moment, and the board silently losing an element
/// there is exactly the wrong time for it.
#[component]
pub fn PlayerDeckView(investigator: InvestigatorId, remaining: usize) -> impl IntoView {
    view! {
        <div class="player-deck">
            <div class="deck-back"></div>
            <span class="deck-count">{remaining}</span>
            <AnchoredControl
                label="Draw".to_string()
                class="draw-control"
                target=OptionTarget::PlayerDeck(investigator)
            />
        </div>
    }
}

/// The scenario's encounter deck, beside the act and agenda: a card back, the
/// remaining count, and a Draw button live exactly while the Mythos step-1.4
/// prompt is up.
///
/// That prompt is an option-less `Confirm`, so it is the **request** anchor —
/// not an option anchor — that puts the button here and tells it apart from the
/// skill-test acknowledge pause, which is the same shape on the wire (ADR 0011).
pub fn encounter_deck_view(game: &GameState) -> impl IntoView {
    let remaining = game.encounter_deck.len();
    let live = use_context::<crate::interaction::ConfirmAnchor>()
        .and_then(|a| a.0.get())
        .is_some_and(|a| a == OptionTarget::EncounterDeck);
    view! {
        <div class="encounter-deck">
            <span class="deck-label">"Encounter"</span>
            <div class="deck-back"></div>
            <span class="deck-count">{remaining}</span>
            <button
                class="encounter-draw"
                class:actionable=live
                disabled=!live
                on:click={
                    #[cfg(target_arch = "wasm32")]
                    { move |_| if live { submit_confirm("Draw encounter card".to_string()) } }
                    #[cfg(not(target_arch = "wasm32"))]
                    { |_| () }
                }
            >
                "Draw"
            </button>
        </div>
    }
}
