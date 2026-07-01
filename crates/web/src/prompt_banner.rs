//! Bottom-fixed prompt banner (interactivity S3, #538): for a live `PickMultiple`
//! prompt, renders its text + a Confirm (submits the `MultiSelect` selection) and,
//! when skippable, a Pass (submits Skip). wasm-only — submits via `OutboundTx`.
//! Other prompt kinds stay in the flat bar until later slices.

use std::collections::BTreeSet;

use game_core::{EngineOutcome, InputKind, InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::interaction::MultiSelect;
use crate::store::use_store;
use crate::transport::OutboundTx;

/// The bottom-fixed multi-select banner. Renders nothing unless a `PickMultiple`
/// prompt is live and a [`MultiSelect`] context is present.
#[component]
pub fn PromptBanner() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();
    let ms = use_context::<MultiSelect>();
    view! {
        {move || {
            let state = store.get();
            let Some(EngineOutcome::AwaitingInput { request, .. }) = state.outcome else {
                return ().into_any();
            };
            if request.kind != InputKind::PickMultiple {
                return ().into_any();
            }
            let Some(ms) = ms.clone() else {
                return ().into_any();
            };
            let selected = ms.selected;
            let prompt = request.prompt.clone();
            let skippable = request.skippable;

            let tx_c = tx.clone();
            let confirm = move |_| {
                if let Some(tx) = tx_c.clone() {
                    let sel: Vec<OptionId> =
                        selected.get_untracked().into_iter().map(OptionId).collect();
                    store.update(|s| {
                        s.pending_label = Some(format!("Commit {} card(s)", sel.len()));
                    });
                    let _ = tx.unbounded_send(ClientMessage::Submit {
                        action: PlayerAction::ResolveInput {
                            response: InputResponse::PickMultiple { selected: sel },
                        },
                    });
                    selected.set(BTreeSet::new());
                }
            };

            let tx_s = tx.clone();
            let pass = move |_| {
                if let Some(tx) = tx_s.clone() {
                    store.update(|s| s.pending_label = Some("Skip".to_string()));
                    let _ = tx.unbounded_send(ClientMessage::Submit {
                        action: PlayerAction::ResolveInput {
                            response: InputResponse::Skip,
                        },
                    });
                }
            };
            let pass_btn =
                skippable.then(|| view! { <button class="pass" on:click=pass>"Pass"</button> });

            view! {
                <div class="prompt-banner">
                    <span class="prompt">{prompt}</span>
                    <button class="confirm" on:click=confirm>"Confirm"</button>
                    {pass_btn}
                </div>
            }
            .into_any()
        }}
    }
}
