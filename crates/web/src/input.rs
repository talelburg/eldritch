//! `AwaitingInput` resolution UI (P6.6, wasm-only). Renders the engine's
//! pending prompt and a control to resolve it.
//!
//! The control is chosen by the request's [`InputKind`](game_core::InputKind)
//! discriminator (#205) — never by inspecting the prompt text or whether
//! `options` is empty:
//!
//! - **`PickSingle`** — one button per [`ChoiceOption`]; click submits
//!   `ResolveInput(PickSingle(id))`.
//! - **`Confirm`** — a single "Confirm" button → `ResolveInput(Confirm)`
//!   (e.g. the Mythos encounter draw).
//!
//! **`PickMultiple`** (mulligan / skill-test commit / hand-size discard) is
//! **not** handled here — it is click-to-select on the board hand cards plus the
//! bottom [`PromptBanner`](crate::prompt_banner::PromptBanner) (#538); this view
//! returns nothing for it.
//!
//! Orthogonally, when `request.skippable` is set (e.g. a non-forced reaction
//! window), a "Skip" button → `ResolveInput(Skip)` renders alongside whichever
//! control the `kind` selected.
//!
//! Nothing renders when the latest outcome is not `AwaitingInput`.

use game_core::{ChoiceOption, EngineOutcome, InputKind, InputResponse, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::store::use_store;
use crate::transport::OutboundTx;

/// Pending-input prompt + commit control. Reads the store reactively;
/// submits via the `OutboundTx` provided by the transport (absent in
/// render-only contexts, so read as `Option`).
///
/// Dispatches on the [`InputRequest`](game_core::InputRequest)'s
/// [`kind`](game_core::InputRequest::kind): a `PickSingle` option-list or a
/// `Confirm` button — plus a "Skip" button when
/// [`skippable`](game_core::InputRequest::skippable) (#205). `PickMultiple` is
/// handled by the [`PromptBanner`](crate::prompt_banner::PromptBanner), not here
/// (#538) — this view returns nothing for it.
// The `PickSingle` / `Confirm` `view!` arms (plus the fallback + the Skip
// control); the length is inherent to the per-kind dispatch, not extractable
// without fighting leptos's closure captures.
#[allow(clippy::too_many_lines)]
#[component]
pub fn AwaitingInputView() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();

    view! {
        {move || {
            let state = store.get();
            let Some(EngineOutcome::AwaitingInput { request, .. }) = state.outcome.clone() else {
                return ().into_any();
            };

            // PickMultiple (mulligan / commit / hand-size discard) is rendered by
            // the bottom prompt banner (#538), not here.
            if request.kind == InputKind::PickMultiple {
                return ().into_any();
            }

            // A Skip/Pass control, rendered (independent of `kind`) whenever the
            // request is skippable — e.g. a non-forced reaction window.
            let skippable = request.skippable;
            let skip_button = {
                let tx = tx.clone();
                move || {
                    if !skippable {
                        return ().into_any();
                    }
                    let tx = tx.clone();
                    view! {
                        <button
                            class="skip"
                            on:click=move |_| {
                                if let Some(tx) = tx.clone() {
                                    store.update(|s| s.pending_label = Some("Skip".to_string()));
                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                        action: PlayerAction::ResolveInput {
                                            response: InputResponse::Skip,
                                        },
                                    });
                                }
                            }
                        >
                            "Skip"
                        </button>
                    }
                    .into_any()
                }
            };

            match request.kind {
                // One button per offered option → ResolveInput(PickSingle(id)).
                InputKind::PickSingle => {
                    let tx = tx.clone();
                    let buttons: Vec<_> = request
                        .options
                        .iter()
                        .cloned()
                        .map(|opt: ChoiceOption| {
                            let ChoiceOption { id, label, .. } = opt;
                            let tx = tx.clone();
                            let header = label.clone();
                            view! {
                                <button
                                    class="option"
                                    on:click=move |_| {
                                        if let Some(tx) = tx.clone() {
                                            store.update(|s| s.pending_label = Some(header.clone()));
                                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                                action: PlayerAction::ResolveInput {
                                                    response: InputResponse::PickSingle(id),
                                                },
                                            });
                                        }
                                    }
                                >
                                    {label}
                                </button>
                            }
                        })
                        .collect();
                    view! {
                        <section class="awaiting-input">
                            <p class="prompt">{request.prompt.clone()}</p>
                            <div class="option-list">{buttons}</div>
                            {skip_button()}
                        </section>
                    }
                    .into_any()
                }
                // A single acknowledge button → ResolveInput(Confirm).
                InputKind::Confirm => {
                    let tx = tx.clone();
                    view! {
                        <section class="awaiting-input">
                            <p class="prompt">{request.prompt.clone()}</p>
                            <button
                                class="confirm"
                                on:click=move |_| {
                                    if let Some(tx) = tx.clone() {
                                        store.update(|s| s.pending_label = Some("Confirm".to_string()));
                                        let _ = tx.unbounded_send(ClientMessage::Submit {
                                            action: PlayerAction::ResolveInput {
                                                response: InputResponse::Confirm,
                                            },
                                        });
                                    }
                                }
                            >
                                "Confirm"
                            </button>
                            {skip_button()}
                        </section>
                    }
                    .into_any()
                }
                // `InputKind` is `#[non_exhaustive]`; a future kind the client
                // doesn't yet render falls back to the prompt + any Skip control.
                _ => view! {
                    <section class="awaiting-input">
                        <p class="prompt">{request.prompt}</p>
                        {skip_button()}
                    </section>
                }
                .into_any(),
            }
        }}
    }
}
