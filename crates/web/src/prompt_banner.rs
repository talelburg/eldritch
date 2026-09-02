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

use game_core::{ChoiceOption, EngineOutcome, InputKind, InputResponse, OptionId, OptionTarget};
use leptos::prelude::*;

use crate::interaction::MultiSelect;
use crate::store::use_store;

/// The bottom-fixed prompt banner. See the module docs for what it renders and
/// what it deliberately does not.
// The per-control `view!` arms; the length is inherent to the control dispatch,
// not extractable without fighting leptos's closure captures.
#[allow(clippy::too_many_lines)]
#[component]
pub fn PromptBanner() -> impl IntoView {
    let store = use_store();
    let ms = use_context::<MultiSelect>();
    view! {
        {move || {
            let state = store.get();
            let Some(EngineOutcome::AwaitingInput { request, .. }) = state.outcome.clone() else {
                return ().into_any();
            };
            let is_multi = request.kind == InputKind::PickMultiple;
            // A decision has its own modal, and that modal is its *sole* surface
            // (#856): the banner renders neither its text nor its branches. The
            // predicate lives with the view it governs, exactly as the skill-test
            // result modal's does below. The banner's other controls stay — it is
            // still the floor, and a Pass with nowhere else to go must not vanish
            // with the text.
            let decision_is_live = crate::decision::modal_is_live(&state);
            // The open-turn menu is the one prompt whose *text* the banner
            // swallows — identified by its anchor rather than by its string (ADR
            // 0011), because "Choose an action" every turn would spend the one
            // persistent surface on noise (#541).
            //
            // Only the text. The banner is the floor, so its controls stay: a
            // turn action whose anchor is `None` — Investigate with no current
            // location, EndTurn off-frame — has no board home, and suppressing
            // the whole banner would make it unreachable rather than merely
            // misplaced.
            let suppress_text = decision_is_live
                || matches!(
                    crate::interaction::prompt_anchor(&state),
                    Some(OptionTarget::TurnControl(_))
                );
            let prompt = (!suppress_text).then(|| request.prompt.clone());

            // Option buttons — the live prompt's **un-anchored** options only;
            // an anchored option renders on its board card and must not render
            // twice (#541). With the flat bar gone this is the only home an
            // un-anchored option has: evaluator `ChooseOne` branches, the
            // skill-substitution pick, a soak point taken by the investigator.
            //
            // A decision's branches are excluded whatever their anchor: they are
            // un-anchored whenever #845 degrades the source, and the modal is
            // carrying them regardless.
            let option_btns: Vec<_> = request
                .options
                .iter()
                .filter(|opt| !decision_is_live && opt.target.is_none())
                .cloned()
                .map(|opt: ChoiceOption| {
                    let ChoiceOption { id, label, .. } = opt;
                    let header = label.clone();
                    let pick = move |_| {
                        crate::controls::submit(InputResponse::PickSingle(id), header.clone());
                    };
                    view! { <button class="banner-option" on:click=pick>{label}</button> }
                })
                .collect();

            // Confirm — PickMultiple only (submits the MultiSelect selection).
            let confirm_btn = is_multi.then(|| ms.clone()).flatten().map(|ms| {
                let selected = ms.selected;
                let confirm = move |_| {
                    let sel: Vec<OptionId> =
                        selected.get_untracked().into_iter().map(OptionId).collect();
                    let label = format!("Commit {} card(s)", sel.len());
                    crate::controls::submit(InputResponse::PickMultiple { selected: sel }, label);
                    selected.set(BTreeSet::new());
                };
                view! { <button class="confirm" on:click=confirm>"Confirm"</button> }
            });

            // Confirm — a fallback for an un-anchored `Confirm` the skill-test
            // result modal is not already carrying its own button for. The
            // acknowledge pause renders on the modal (which is what makes it
            // un-dismissible except by that button); anything *else* un-anchored
            // and option-less would otherwise be unreachable.
            let confirm_fallback = (request.kind == InputKind::Confirm
                && request.target.is_none()
                && !crate::skill_test_result::modal_is_live(&state))
            .then(|| {
                let confirm =
                    move |_| crate::controls::submit(InputResponse::Confirm, "Confirm");
                view! { <button class="confirm" on:click=confirm>"Confirm"</button> }
            });

            // Pass — whenever the request is skippable.
            let pass_btn = request.skippable.then(|| {
                let pass = move |_| crate::controls::submit(InputResponse::Skip, "Skip");
                view! { <button class="pass" on:click=pass>"Pass"</button> }
            });

            // Nothing to say and nothing to offer — the ordinary open turn, where
            // every option sits on a board surface. Render no bar at all rather
            // than an empty one (#541).
            if prompt.is_none()
                && option_btns.is_empty()
                && confirm_btn.is_none()
                && confirm_fallback.is_none()
                && pass_btn.is_none()
            {
                return ().into_any();
            }

            view! {
                <div class="prompt-banner">
                    {prompt.map(|p| view! { <span class="prompt">{p}</span> })}
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
