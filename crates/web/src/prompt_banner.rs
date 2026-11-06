//! The prompt's **strips**: where this account answers a live prompt for one
//! of its investigators.
//!
//! Which strips exist is [`prompt_layout::layout`]'s to say. The dock renders
//! the strip of the open tab's investigator, headed with their name, when the
//! prompt asks them and you hold them, and the lead strip renders above the
//! tabs, labelled "★ As lead investigator", when you hold the lead and the
//! lead decides for the table. This module renders what goes inside each.
//!
//! A strip names the prompt, unless it is the open-turn menu, which the engine
//! anchors to [`TurnControl`](game_core::engine::OptionTarget::TurnControl) so a
//! strip can suppress its "Choose an action" noise **structurally** rather than
//! by matching the prompt string (ADR 0011), or a decision whose modal shows
//! the text itself.
//!
//! It carries the controls that belong nowhere on the board: a live prompt's
//! **un-anchored** options and those anchored to a card committed to the skill
//! test (no board surface renders those yet), a `PickMultiple` commit's Confirm
//! (submitting the `MultiSelect` selection), a skippable prompt's Pass, and a
//! fallback Confirm for an un-anchored `Confirm` the skill-test result modal is
//! not already carrying. In a window several answer, each strip carries only
//! its own investigator's options and a Pass for them alone (ADR 0019). A strip
//! with nothing to say and nothing to offer renders nothing. wasm-only —
//! submits via `OutboundTx`.

use std::collections::BTreeSet;

use game_core::action::InputResponse;
use game_core::engine::{
    ChoiceOption, InputKind, InputRequest, OptionId, OptionTarget, PickCount, Responder,
};
use game_core::state::investigator::InvestigatorId;
use leptos::prelude::*;

use crate::interaction::MultiSelect;
use crate::prompt_layout::{AboveBoard, OpenTab};
use crate::store::ClientState;
use crate::{controls, decision, prompt_layout, skill_test_result, store};

/// `investigator`'s strip, headed with their name: the dock renders it while
/// their tab is open. It renders nothing unless the live prompt asks them and
/// this account holds them, or when it has nothing to carry.
#[component]
pub fn PromptStrip(investigator: InvestigatorId) -> impl IntoView {
    let store = store::use_store();
    let ms = use_context::<MultiSelect>();
    move || {
        let state = store.get();
        // `layout` decides which strips exist; `answerable_request` only
        // supplies what goes in them. The two agree, because a strip means
        // this account holds the investigator, so it can answer the prompt.
        let request = state.answerable_request()?;
        // In prompt order across all of this account's strips, so the first
        // carries what the store answers as the first, whichever tab is open.
        let strips = prompt_layout::layout(&state, OpenTab::default()).strips();
        let place = strips.iter().position(|&id| id == investigator)?;
        let share = match request.responder {
            Responder::Several => Share::Own {
                investigator,
                first: place == 0,
            },
            _ => Share::All,
        };
        let controls = strip_contents(&state, &request, ms.clone(), share)?;
        let name = state.investigator_name(investigator);
        Some(view! {
            <div class="prompt-strip">
                <span class="strip-name">{name}</span>
                {controls}
            </div>
        })
    }
}

/// The lead's strip, above the board: the account holding the lead
/// investigator answers here when the lead decides for the table.
#[component]
pub fn LeadStrip() -> impl IntoView {
    let store = store::use_store();
    let ms = use_context::<MultiSelect>();
    move || {
        let state = store.get();
        let Some(AboveBoard::LeadStrip(_)) =
            prompt_layout::layout(&state, OpenTab::default()).above_board
        else {
            return None;
        };
        // `layout` decided the lead strip exists; `answerable_request` only
        // supplies its contents. The two agree, because a lead strip means
        // this account holds the lead, so it can answer the prompt.
        let request = state.answerable_request()?;
        let controls = strip_contents(&state, &request, ms.clone(), Share::All)?;
        Some(view! {
            <div class="lead-strip">
                <span class="strip-name">"★ As lead investigator"</span>
                {controls}
            </div>
        })
    }
}

/// Which of the prompt's controls one strip carries.
#[derive(Debug, Clone, Copy)]
enum Share {
    /// All of them: the strip is the prompt's only one on this client.
    All,
    /// In a window several answer, `investigator`'s options and a Pass for
    /// them alone. The `first` strip also carries the options naming no
    /// investigator and any Confirm, which the store answers as the first of
    /// your investigators holding an option.
    Own {
        investigator: InvestigatorId,
        first: bool,
    },
}

impl Share {
    /// Whether this strip carries `option`.
    fn carries(self, option: &ChoiceOption) -> bool {
        match self {
            Share::All => true,
            Share::Own {
                investigator,
                first,
            } => option.investigator.map_or(first, |own| own == investigator),
        }
    }

    /// Whether this strip carries the prompt's Confirm.
    fn carries_confirm(self) -> bool {
        match self {
            Share::All => true,
            Share::Own { first, .. } => first,
        }
    }
}

/// What one strip carries of `request`, the live prompt as this account may
/// answer it, or `None` when it has nothing to say and nothing to offer.
fn strip_contents(
    state: &ClientState,
    request: &InputRequest,
    ms: Option<MultiSelect>,
    share: Share,
) -> Option<AnyView> {
    // A decision has its own modal, and that modal is its *sole* surface: the
    // strip renders neither its text nor its branches. The predicate lives
    // with the view it governs, exactly as the skill-test result modal's does
    // below. The strip's other controls stay — a Pass with nowhere else to go
    // must not vanish with the text.
    let decision_is_live = decision::modal_is_live(state);
    // The open-turn menu is the one prompt whose *text* the strip swallows —
    // identified by its anchor rather than by its string (ADR 0011), because
    // "Choose an action" every turn would spend the one persistent surface on
    // noise.
    //
    // Only the text. Its controls stay: a turn action whose anchor is `None` —
    // Investigate with no current location, EndTurn off-frame — has no board
    // home, and suppressing the whole strip would make it unreachable rather
    // than merely misplaced.
    //
    // `suppresses_text` is the one definition of both cases, which the waiting
    // strip reads too.
    let suppress_text = prompt_layout::suppresses_text(state, request);
    let prompt = (!suppress_text).then(|| request.prompt.clone());
    // A prompt inside a skill test's verdict, before the verdict is logged —
    // Lucky! 01080's `when` cell — says how the test stands, since the result
    // modal comes only after it.
    let verdict = prompt
        .is_some()
        .then(|| skill_test_result::provisional_verdict(state))
        .flatten();

    // Option buttons — the live prompt's **un-anchored** options only; an
    // anchored option renders on its board card and must not render twice.
    // This is the only home an un-anchored option has: evaluator `ChooseOne`
    // branches, the skill-substitution pick, a soak point taken by the
    // investigator.
    //
    // A decision's branches are excluded whatever their anchor: they are
    // un-anchored whenever their source has left play and the engine degrades
    // the anchor, and the modal is carrying them regardless.
    //
    // A committed card's option (the ST.7 result-order pick) is anchored, but
    // no board surface renders the committed cards yet, so it homes here too,
    // under the engine's label, which names the card.
    // TODO(#1322): drop the `CommittedCard` arm once a committed-card surface
    // renders the option on its card.
    let option_btns: Vec<_> = request
        .options
        .iter()
        .filter(|opt| {
            !decision_is_live
                && share.carries(opt)
                && matches!(opt.target, None | Some(OptionTarget::CommittedCard { .. }))
        })
        .cloned()
        .map(|opt: ChoiceOption| {
            let ChoiceOption { id, label, .. } = opt;
            let pick = move |_| controls::submit(InputResponse::PickSingle(id));
            view! { <button class="banner-option" on:click=pick>{label}</button> }
        })
        .collect();

    // Confirm — PickMultiple only (submits the MultiSelect selection).
    let confirm_btn = match request.kind {
        InputKind::PickMultiple(count) if share.carries_confirm() => {
            ms.map(|ms| multi_select_confirm(count, ms.selected))
        }
        _ => None,
    };

    // Confirm — a fallback for an un-anchored `Confirm` the skill-test result
    // modal is not already carrying its own button for. The acknowledge pause
    // renders on the modal (which is what makes it un-dismissible except by
    // that button); anything *else* un-anchored and option-less would
    // otherwise be unreachable.
    let confirm_fallback = (share.carries_confirm()
        && request.kind == InputKind::Confirm
        && request.target.is_none()
        && !skill_test_result::modal_is_live(state))
    .then(|| {
        let confirm = move |_| controls::submit(InputResponse::Confirm);
        view! { <button class="confirm" on:click=confirm>"Confirm"</button> }
    });

    // Pass — whenever the request is skippable. In a window several answer,
    // each investigator holding an option passes separately (ADR 0019), so the
    // Pass names this strip's investigator; anywhere else the store names the
    // one investigator the prompt asks.
    let pass_btn = request.skippable.then(|| {
        let pass = move |_| match share {
            Share::Own { investigator, .. } => {
                controls::submit_as(investigator, InputResponse::Skip);
            }
            Share::All => controls::submit(InputResponse::Skip),
        };
        view! { <button class="pass" on:click=pass>"Pass"</button> }
    });

    // Nothing to say and nothing to offer — the ordinary open turn, where
    // every option sits on a board surface. Render no strip at all rather than
    // a heading alone.
    if prompt.is_none()
        && option_btns.is_empty()
        && confirm_btn.is_none()
        && confirm_fallback.is_none()
        && pass_btn.is_none()
    {
        return None;
    }

    Some(
        view! {
            {prompt.map(|p| view! { <span class="prompt">{p}</span> })}
            {verdict.map(|v| view! { <span class="provisional-verdict">{v}</span> })}
            {option_btns}
            {confirm_btn}
            {confirm_fallback}
            {pass_btn}
        }
        .into_any(),
    )
}

/// A `PickMultiple`'s Confirm: submits the `selected` options, and is disabled
/// until the selection is a size `count` admits. The engine rejects any other
/// size (a hand-size discard of exactly the excess), so an enabled Confirm
/// there would only bounce.
fn multi_select_confirm(count: PickCount, selected: RwSignal<BTreeSet<u32>>) -> impl IntoView {
    let confirm = move |_| {
        let sel: Vec<OptionId> = selected.get_untracked().into_iter().map(OptionId).collect();
        controls::submit(InputResponse::PickMultiple { selected: sel });
        selected.set(BTreeSet::new());
    };
    let out_of_range = move || !count.admits(selected.with(BTreeSet::len));
    view! { <button class="confirm" disabled=out_of_range on:click=confirm>"Confirm"</button> }
}
