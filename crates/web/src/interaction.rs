//! Board interactivity routing (#536): map an `AwaitingInput`'s options to the
//! board entity each acts on (via S0's `OptionTarget`), plus the `ContextMenu`
//! that renders a chosen entity's options. The routing fns are pure and
//! native-tested; `ContextMenu` is wasm-only (it submits via the wasm-only
//! `OutboundTx`).

use game_core::{ChoiceOption, EngineOutcome, OptionTarget};
use leptos::prelude::Signal;

use crate::store::ClientState;

/// The live prompt's offered options — the `AwaitingInput` request's `options`,
/// else empty (`Done` / `Rejected` / no outcome). Pure.
#[must_use]
pub fn pending_options(state: &ClientState) -> Vec<ChoiceOption> {
    match &state.outcome {
        Some(EngineOutcome::AwaitingInput { request, .. }) => request.options.clone(),
        _ => Vec::new(),
    }
}

/// The options anchored to `target`, in offered order. Pure; a linear scan
/// (option counts are tiny, so `OptionTarget` needs no `Hash`).
#[must_use]
pub fn options_for(options: &[ChoiceOption], target: OptionTarget) -> Vec<ChoiceOption> {
    options
        .iter()
        .filter(|o| o.target == target)
        .cloned()
        .collect()
}

/// Context newtype carrying the derived pending-options signal, so any entity
/// reads it without prop-drilling. A distinct type so it can't collide with
/// other `Signal` contexts.
#[derive(Clone)]
pub struct PendingOptions(pub Signal<Vec<ChoiceOption>>);

/// A popover of a board entity's offered options (#536, #537). When `open` is
/// `Some((x, y))`, renders a full-screen transparent `.menu-backdrop` (click →
/// close) and a `.context-menu` positioned `fixed` at viewport coords `(x, y)`
/// (so it escapes any `overflow`/positioning ancestor); a click submits
/// `ResolveInput(PickSingle(id))` and closes. wasm-only — submits via the
/// wasm-only `OutboundTx`.
#[cfg(target_arch = "wasm32")]
#[leptos::component]
pub fn ContextMenu(
    options: Vec<ChoiceOption>,
    open: leptos::prelude::RwSignal<Option<(i32, i32)>>,
) -> impl leptos::prelude::IntoView {
    use leptos::prelude::*;

    use game_core::{InputResponse, PlayerAction};
    use protocol::ClientMessage;

    use crate::store::use_store;
    use crate::transport::OutboundTx;

    let store = use_store();
    let tx = use_context::<OutboundTx>();

    view! {
        {move || {
            let Some((x, y)) = open.get() else {
                return ().into_any();
            };
            let tx = tx.clone();
            let buttons: Vec<_> = options
                .iter()
                .cloned()
                .map(|opt| {
                    let ChoiceOption { id, label, .. } = opt;
                    let tx = tx.clone();
                    let header = label.clone();
                    view! {
                        <button
                            class="menu-item"
                            on:click=move |ev| {
                                ev.stop_propagation();
                                if let Some(tx) = tx.clone() {
                                    store.update(|s| s.pending_label = Some(header.clone()));
                                    let _ = tx.unbounded_send(ClientMessage::Submit {
                                        action: PlayerAction::ResolveInput {
                                            response: InputResponse::PickSingle(id),
                                        },
                                    });
                                }
                                open.set(None);
                            }
                        >
                            {label}
                        </button>
                    }
                })
                .collect();
            let style = format!("left:{x}px;top:{y}px;");
            view! {
                <div
                    class="menu-backdrop"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open.set(None);
                    }
                ></div>
                <div class="context-menu" style=style>{buttons}</div>
            }
            .into_any()
        }}
    }
}

/// The interactive trigger for a board entity's context menu (#537), wasm-only.
/// A transparent hit-layer covering the anchor captures the open click's viewport
/// coords into `open`; the [`ContextMenu`] renders there. Embedded by each entity
/// under `#[cfg(target_arch = "wasm32")]` so no `web_sys` touches the host build;
/// the anchor supplies the `actionable` glow class + `position: relative`.
#[cfg(target_arch = "wasm32")]
pub fn menu_layer(
    options: Vec<ChoiceOption>,
    open: leptos::prelude::RwSignal<Option<(i32, i32)>>,
) -> impl leptos::prelude::IntoView {
    use leptos::prelude::*;
    view! {
        <div
            class="menu-hit"
            on:click=move |ev: web_sys::MouseEvent| {
                open.set(Some((ev.client_x(), ev.client_y())));
            }
        ></div>
        <ContextMenu options=options open=open/>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::{EnemyId, LocationId};
    use game_core::OptionId;

    fn opt(id: u32, target: OptionTarget) -> ChoiceOption {
        ChoiceOption::new(OptionId(id), format!("opt{id}"), target)
    }

    #[test]
    fn pending_options_empty_when_not_awaiting() {
        assert!(pending_options(&ClientState::default()).is_empty());
    }

    #[test]
    fn pending_options_returns_the_awaiting_requests_options() {
        let state = ClientState {
            outcome: Some(
                game_core::test_support::fixtures::awaiting_pick_single_with(
                    "x",
                    vec![opt(0, OptionTarget::Location(LocationId(10)))],
                ),
            ),
            ..Default::default()
        };
        assert_eq!(pending_options(&state).len(), 1);
    }

    #[test]
    fn options_for_returns_only_the_matching_anchor() {
        let opts = vec![
            opt(0, OptionTarget::Location(LocationId(10))),
            opt(1, OptionTarget::Enemy(EnemyId(7))),
            opt(2, OptionTarget::Global),
            opt(3, OptionTarget::Location(LocationId(11))),
        ];
        let got = options_for(&opts, OptionTarget::Location(LocationId(10)));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, OptionId(0));
    }
}
