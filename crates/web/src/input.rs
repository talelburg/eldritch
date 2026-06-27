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
//! - **`PickMultiple`** — the hand-card commit UI for skill-test commit /
//!   mulligan / hand-size discard → `ResolveInput(PickMultiple { selected })`.
//!
//! Orthogonally, when `request.skippable` is set (e.g. a non-forced reaction
//! window), a "Skip" button → `ResolveInput(Skip)` renders alongside whichever
//! control the `kind` selected.
//!
//! Nothing renders when the latest outcome is not `AwaitingInput`.

use std::collections::BTreeSet;

use game_core::state::GameState;
use game_core::{ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;

use crate::store::use_store;
use crate::transport::OutboundTx;

/// Pending-input prompt + commit control. Reads the store reactively;
/// submits via the `OutboundTx` provided by the transport (absent in
/// render-only contexts, so read as `Option`).
///
/// Dispatches on the [`InputRequest`](game_core::InputRequest)'s
/// [`kind`](game_core::InputRequest::kind): a `PickSingle` option-list, a
/// `Confirm` button, or the `PickMultiple` hand-card commit UI — plus a "Skip"
/// button when [`skippable`](game_core::InputRequest::skippable) (#205).
// Three parallel `view!` rendering arms (PickSingle / Confirm / PickMultiple)
// plus the Skip control; the length is inherent to the per-kind dispatch, not
// extractable without fighting leptos's closure captures.
#[allow(clippy::too_many_lines)]
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
                // Hand-card multi-select → ResolveInput(PickMultiple { selected }).
                // The host derives candidates from the prompted hand, treating
                // each OptionId(i) as hand index i (see InputRequest::pick_multiple).
                InputKind::PickMultiple => {
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
                                        {crate::names::card_name(&code)}
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
                            <button class="commit" on:click=on_commit>"Confirm"</button>
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

/// The prompted investigator's hand as card codes (empty when no
/// investigator is being prompted).
///
/// Falls back to the prompted investigator when there is no active
/// investigator: during the setup mulligan loop
/// ([`GameState::current_mulligan`], #348) and during the upkeep hand-size
/// discard ([`GameState::current_hand_size_discard`], #468) `active_investigator`
/// is not set, but the `PickMultiple` redraw/discard still targets that
/// investigator's hand.
///
/// Solo-scope assumption: the skill-test performer equals
/// `active_investigator`. The authoritative "whose hand commits" is
/// `in_flight_skill_test.investigator`; the two coincide in solo but need
/// not in a delegated/multiplayer test, so input routing is revisited in
/// #205.
fn active_hand(game: &GameState) -> Vec<game_core::state::CardCode> {
    game.active_investigator
        .or_else(|| game.current_mulligan())
        .or_else(|| game.current_hand_size_discard())
        .and_then(|id| game.investigators.get(&id))
        .map(|inv| inv.hand.clone())
        .unwrap_or_default()
}
