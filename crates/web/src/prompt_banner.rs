//! The prompt banner (interactivity S3–S6, #538/#539/#541) — **the floor** now
//! that the flat `.action-bar` is gone.
//!
//! It renders whenever an `AwaitingInput` is live, with one exception: the
//! open-turn menu, which the engine anchors to
//! [`TurnControl`](game_core::OptionTarget::TurnControl) so the banner can
//! suppress its "Choose an action" noise **structurally** rather than by matching
//! the prompt string (ADR 0011). Every other prompt gets at least its text here,
//! so a prompt the client does not specifically home still says the engine is
//! waiting (#770 stays open; this is a non-regression floor, not its fix).
//!
//! It stays the home for the controls that belong nowhere on the board: a live
//! prompt's **un-anchored** options, a `PickMultiple` commit's Confirm (submitting
//! the `MultiSelect` selection), any skippable window's Pass, and a fallback
//! Confirm for an un-anchored `Confirm` the skill-test result modal is not already
//! carrying. wasm-only — submits via `OutboundTx`.

use std::collections::BTreeSet;

use game_core::{
    ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, OptionTarget, PlayerAction,
};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::interaction::MultiSelect;
use crate::store::use_store;
use crate::transport::OutboundTx;

/// The bottom-fixed prompt banner. See the module docs for what it renders and
/// what it deliberately does not.
// The per-control `view!` arms; the length is inherent to the control dispatch,
// not extractable without fighting leptos's closure captures.
#[allow(clippy::too_many_lines)]
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
            // The one prompt the banner stays silent about: the open-turn menu,
            // identified by its anchor rather than by its text (ADR 0011). Its
            // options all live on board surfaces, and "Choose an action" every
            // turn would spend the one persistent surface on noise (#541).
            if matches!(request.target, Some(OptionTarget::TurnControl(_))) {
                return ().into_any();
            }
            let prompt = request.prompt.clone();

            // Option buttons — the live prompt's **un-anchored** options only;
            // an anchored option renders on its board card and must not render
            // twice (#541). With the flat bar gone this is the only home an
            // un-anchored option has: evaluator `ChooseOne` branches, the
            // skill-substitution pick, a soak point taken by the investigator.
            let option_btns: Vec<_> = request
                .options
                .iter()
                .filter(|opt| opt.target.is_none())
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

            // Confirm — a fallback for an un-anchored `Confirm` the skill-test
            // result modal is not already carrying its own button for. The
            // acknowledge pause renders on the modal (which is what makes it
            // un-dismissible except by that button); anything *else* un-anchored
            // and option-less would otherwise be unreachable.
            let modal_live = crate::skill_test_result::summarize(
                &state.last_events,
                state.last_skill_test_difficulty,
            )
            .is_some();
            let confirm_fallback = (request.kind == InputKind::Confirm
                && request.target.is_none()
                && !modal_live)
                .then(|| {
                    let tx = tx.clone();
                    let confirm = move |_| {
                        if let Some(tx) = tx.clone() {
                            store.update(|s| s.pending_label = Some("Confirm".to_string()));
                            let _ = tx.unbounded_send(ClientMessage::Submit {
                                action: PlayerAction::ResolveInput {
                                    response: InputResponse::Confirm,
                                },
                            });
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
                    {confirm_fallback}
                    {pass_btn}
                </div>
            }
            .into_any()
        }}
    }
}
