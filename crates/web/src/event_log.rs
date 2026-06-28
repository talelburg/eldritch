//! Event-log panel (#505): a read-only, accumulating view of the game's events,
//! left of the board, newest at the bottom, grouped per submitted action.

use game_core::{InputRequest, InputResponse};

/// The event-log header for a submitted response, given the prompt it answered.
///
/// - `PickSingle(id)` → that option's `label` (fallback `"Pick <n>"` if absent).
/// - `Confirm`        → `"Confirm"`.
/// - `Skip`           → `"Skip"`.
/// - `PickMultiple`   → `"Commit <n> card(s)"`.
#[allow(dead_code)]
pub(crate) fn response_label(request: &InputRequest, response: &InputResponse) -> String {
    match response {
        InputResponse::PickSingle(id) => request
            .options
            .iter()
            .find(|o| o.id == *id)
            .map_or_else(|| format!("Pick {}", id.0), |o| o.label.clone()),
        InputResponse::Confirm => "Confirm".to_string(),
        InputResponse::Skip => "Skip".to_string(),
        InputResponse::PickMultiple { selected } => {
            format!("Commit {} card(s)", selected.len())
        }
        // `InputResponse` is `#[non_exhaustive]`; a future variant gets a generic
        // header rather than failing to compile.
        _ => "(action)".to_string(),
    }
}

use leptos::prelude::*;

use crate::store::use_store;

/// Read-only event log, left of the board. Renders every accumulated `LogBatch`
/// oldest-first (newest at the bottom); a header line per batch then one Debug
/// line per event. On wasm, auto-scrolls to the bottom as the log grows.
#[component]
pub fn EventLogView() -> impl IntoView {
    let store = use_store();
    let scroll_ref = NodeRef::<leptos::html::Div>::new();

    // Auto-scroll to the newest line whenever the batch count changes.
    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            let _ = store.with(|s| s.log.len());
            if let Some(el) = scroll_ref.get() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    }

    let batches = move || {
        store
            .get()
            .log
            .into_iter()
            .map(|batch| {
                let events: Vec<_> = batch
                    .events
                    .iter()
                    .map(|e| view! { <div class="log-event">{format!("{e:?}")}</div> })
                    .collect();
                view! {
                    <div class="log-batch">
                        <div class="log-action">{format!("▸ {}", batch.header)}</div>
                        {events}
                    </div>
                }
            })
            .collect::<Vec<_>>()
    };

    view! {
        <aside class="event-log">
            <h2>"Event log"</h2>
            <div class="log-scroll" node_ref=scroll_ref>
                {batches}
            </div>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::engine::OptionId;
    use game_core::ChoiceOption;

    fn request_with_options(opts: Vec<(u32, &str)>) -> InputRequest {
        let options = opts
            .into_iter()
            .map(|(i, l)| ChoiceOption {
                id: OptionId(i),
                label: l.to_string(),
            })
            .collect();
        InputRequest::pick_single("choose", options)
    }

    #[test]
    fn pick_single_uses_the_chosen_option_label() {
        let req = request_with_options(vec![(0, "Move to Cellar"), (1, "Play 01059 from hand")]);
        let label = response_label(&req, &InputResponse::PickSingle(OptionId(1)));
        assert_eq!(label, "Play 01059 from hand");
    }

    #[test]
    fn pick_single_unknown_id_falls_back() {
        let req = request_with_options(vec![(0, "Move to Cellar")]);
        let label = response_label(&req, &InputResponse::PickSingle(OptionId(7)));
        assert_eq!(label, "Pick 7");
    }

    #[test]
    fn confirm_skip_and_commit_have_fixed_labels() {
        let req = request_with_options(vec![]);
        assert_eq!(response_label(&req, &InputResponse::Confirm), "Confirm");
        assert_eq!(response_label(&req, &InputResponse::Skip), "Skip");
        assert_eq!(
            response_label(
                &req,
                &InputResponse::PickMultiple {
                    selected: vec![OptionId(0), OptionId(1)]
                }
            ),
            "Commit 2 card(s)"
        );
    }
}
