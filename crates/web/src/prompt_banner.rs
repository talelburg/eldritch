//! Bottom-fixed prompt banner (interactivity S3/S4, #538/#539). Renders its text
//! plus the relevant controls for a live `PickMultiple` commit (Confirm, submitting
//! the `MultiSelect` selection) or any **skippable** window (Pass, submitting Skip)
//! — a `PickMultiple` that is also skippable gets both. wasm-only — submits via
//! `OutboundTx`. Other prompts (open-turn `PickSingle`, encounter `Confirm`) stay
//! in the flat bar until later slices.

use std::collections::BTreeSet;

use game_core::{
    ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, OptionTarget, PlayerAction,
};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::interaction::MultiSelect;
use crate::store::use_store;
use crate::transport::OutboundTx;

/// The bottom-fixed prompt banner. Renders nothing unless the live prompt is a
/// `PickMultiple` commit or a skippable window; for a skippable window it also
/// renders the window's **un-anchored** (`Global`) `PickSingle` options as buttons
/// (#549/#540), so an option reachable nowhere else has a home (anchored options
/// live on their board cards).
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
            let is_multi = request.kind == InputKind::PickMultiple;
            // Rendered for a multi-select commit or any skippable window (#539);
            // other prompts (open-turn PickSingle, encounter Confirm) stay in the bar.
            if !is_multi && !request.skippable {
                return ().into_any();
            }
            let prompt = request.prompt.clone();

            // Option buttons — a skippable window's **un-anchored** (`Global`)
            // `PickSingle` options only; anchored options have board homes (S5,
            // #540). This still homes a genuinely-`Global`/`Board` window option
            // that lives nowhere else (the catch-all the bar retirement relies on).
            let option_btns: Vec<_> = request
                .options
                .iter()
                .filter(|opt| opt.target == OptionTarget::Global)
                .cloned()
                .map(|opt: ChoiceOption| {
                    let ChoiceOption { id, label, .. } = opt;
                    let tx = tx.clone();
                    let header = label.clone();
                    let submit = move |_| {
                        if let Some(tx) = tx.clone() {
                            store.update(|s| s.pending_label = Some(header.clone()));
                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                action: PlayerAction::ResolveInput {
                                    response: InputResponse::PickSingle(id),
                                },
                            });
                        }
                    };
                    view! { <button class="banner-option" on:click=submit>{label}</button> }
                })
                .collect();

            // Confirm — PickMultiple only (submits the MultiSelect selection).
            let confirm_btn = is_multi.then(|| ms.clone()).flatten().map(|ms| {
                let selected = ms.selected;
                let tx = tx.clone();
                let confirm = move |_| {
                    if let Some(tx) = tx.clone() {
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
                view! { <button class="confirm" on:click=confirm>"Confirm"</button> }
            });

            // Pass — whenever the request is skippable.
            let pass_btn = request.skippable.then(|| {
                let tx = tx.clone();
                let pass = move |_| {
                    if let Some(tx) = tx.clone() {
                        store.update(|s| s.pending_label = Some("Skip".to_string()));
                        let _ = tx.unbounded_send(ClientMessage::Submit {
                            action: PlayerAction::ResolveInput {
                                response: InputResponse::Skip,
                            },
                        });
                    }
                };
                view! { <button class="pass" on:click=pass>"Pass"</button> }
            });

            view! {
                <div class="prompt-banner">
                    <span class="prompt">{prompt}</span>
                    {option_btns}
                    {confirm_btn}
                    {pass_btn}
                </div>
            }
            .into_any()
        }}
    }
}
