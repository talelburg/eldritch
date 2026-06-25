//! `AwaitingInput` resolution UI (P6.6, wasm-only). Renders the engine's
//! pending prompt and a control to resolve it.
//!
//! Two rendering branches:
//!
//! - **`PickSingle` option-list** (`request.options` non-empty, #447): one
//!   button per [`ChoiceOption`]; click submits
//!   `ResolveInput(PickSingle(id))`.
//! - **`PickMultiple` commit window** (`request.options` empty): the
//!   legacy hand-card commit UI for skill-test commit / mulligan prompts
//!   (P6.6 / #205).
//!
//! Nothing renders when the latest outcome is not `AwaitingInput`.

use std::collections::BTreeSet;

use game_core::state::GameState;
use game_core::{ChoiceOption, EngineOutcome, InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::store::use_store;
use crate::transport::OutboundTx;

/// Pending-input prompt + commit control. Reads the store reactively;
/// submits via the `OutboundTx` provided by the transport (absent in
/// render-only contexts, so read as `Option`).
///
/// Dispatches to one of two rendering branches based on whether the
/// [`InputRequest`](game_core::InputRequest) carries structured
/// [`options`](game_core::InputRequest::options):
///
/// - Non-empty `options` → [`PickSingle`](InputResponse::PickSingle)
///   option-list (one button per option, #447).
/// - Empty `options` → [`PickMultiple`](InputResponse::PickMultiple)
///   hand-card commit UI (skill-test commit / mulligan, #205).
#[component]
pub fn AwaitingInputView() -> impl IntoView {
    let store = use_store();
    let tx = use_context::<OutboundTx>();
    // One prompt is live at a time in solo, so a single component-lived
    // selection signal suffices. It is cleared after a commit; it is NOT
    // cleared when an `AwaitingInput` is dismissed without committing
    // (server abandons a skill test), which the toy scenario never does —
    // revisit if a path can present a second prompt without an intervening
    // commit.
    let selected = RwSignal::new(BTreeSet::<u32>::new());

    view! {
        {move || {
            let state = store.get();
            let (Some(EngineOutcome::AwaitingInput { request, .. }), Some(game)) =
                (state.outcome.clone(), state.game.clone())
            else {
                return ().into_any();
            };

            // Branch 1: structured PickSingle option-list (#447).
            if !request.options.is_empty() {
                let tx = tx.clone();
                let buttons: Vec<_> = request
                    .options
                    .iter()
                    .cloned()
                    .map(|opt: ChoiceOption| {
                        let ChoiceOption { id, label } = opt;
                        let tx = tx.clone();
                        view! {
                            <button
                                class="option"
                                on:click=move |_| {
                                    if let Some(tx) = tx.clone() {
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
                return view! {
                    <section class="awaiting-input">
                        <p class="prompt">{request.prompt.clone()}</p>
                        <div class="option-list">{buttons}</div>
                    </section>
                }
                .into_any();
            }

            // Branch 2: legacy PickMultiple hand-card commit window (skill-test
            // commit / mulligan). `request.options` is empty for all prompt-only
            // callers that have not yet migrated to the structured contract.
            let cards: Vec<_> = active_hand(&game)
                .into_iter()
                .enumerate()
                .map(|(idx, code)| {
                    let i = u32::try_from(idx).expect("hand fits in u32");
                    view! {
                        <li>
                            <button
                                class="hand-card"
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

            let tx = tx.clone();
            let on_commit = move |_| {
                let selected_ids: Vec<OptionId> =
                    selected.get().into_iter().map(OptionId).collect();
                if let Some(tx) = tx.clone() {
                    let _ = tx.unbounded_send(ClientMessage::Submit {
                        action: PlayerAction::ResolveInput {
                            response: InputResponse::PickMultiple {
                                selected: selected_ids,
                            },
                        },
                    });
                }
                selected.set(BTreeSet::new());
            };

            view! {
                <section class="awaiting-input">
                    <p class="prompt">{request.prompt}</p>
                    <ul class="commit-hand">{cards}</ul>
                    <button class="commit" on:click=on_commit>"Commit"</button>
                </section>
            }
            .into_any()
        }}
    }
}

/// The prompted investigator's hand as card-code strings (empty when no
/// investigator is being prompted).
///
/// Falls back to the setup mulligan's prompted investigator
/// ([`GameState::current_mulligan`]) when there is no active investigator:
/// during the setup mulligan loop `active_investigator` is not yet set, but
/// the `PickMultiple` redraw still targets that investigator's hand (#348).
///
/// Solo-scope assumption: the skill-test performer equals
/// `active_investigator`. The authoritative "whose hand commits" is
/// `in_flight_skill_test.investigator`; the two coincide in solo but need
/// not in a delegated/multiplayer test, so input routing is revisited in
/// #205.
fn active_hand(game: &GameState) -> Vec<String> {
    game.active_investigator
        .or_else(|| game.current_mulligan())
        .and_then(|id| game.investigators.get(&id))
        .map(|inv| inv.hand.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}
