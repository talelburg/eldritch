//! The root Leptos component for the Eldritch web client.

use std::collections::BTreeSet;

use leptos::prelude::*;

use crate::board::BoardView;
#[cfg(target_arch = "wasm32")]
use crate::decision::DecisionView;
use crate::decision::{self, DecisionLive};
use crate::event_log::EventLogView;
use crate::interaction::{self, ConfirmAnchor, MultiSelect, PendingOptions};
#[cfg(target_arch = "wasm32")]
use crate::picker::PickerView;
#[cfg(target_arch = "wasm32")]
use crate::prompt_banner::PromptBanner;
#[cfg(target_arch = "wasm32")]
use crate::sign_in::SignInView;
#[cfg(target_arch = "wasm32")]
use crate::skill_test_result::SkillTestResultView;
use crate::status_bar::StatusBarView;
use crate::store::{self, StoreSignal};
#[cfg(target_arch = "wasm32")]
use crate::transport;
use crate::turn_tracker::TurnTrackerView;
#[cfg(target_arch = "wasm32")]
use crate::unresumable::UnresumableView;
#[cfg(target_arch = "wasm32")]
use crate::version_mismatch::VersionMismatchView;

#[component]
pub fn App() -> impl IntoView {
    let store = store::provide_store();

    provide_board_contexts(store);

    // Spawn the browser transport only on wasm; native/headless-reducer
    // builds render from a signal that tests drive directly.
    #[cfg(target_arch = "wasm32")]
    {
        transport::start(store);
    }

    view! {
        <main>
            <header class="app-header">
                <h1>"Eldritch"</h1>
                <StatusBarView/>
            </header>
            <div class="layout">
                <EventLogView/>
                <MainColumn/>
                <TurnTrackerView/>
            </div>
        </main>
    }
}

/// Provide the board's derived contexts from `store`.
///
/// They are the live prompt's options, the confirm anchor, whether a decision
/// modal is up, and the multi-select state with the effect that clears it. The
/// store, the card registry and the outbound channel are the caller's.
///
/// `App` calls it, and so does the web tests' `mount_board`, so a context the
/// app adds here reaches every board test without a test being edited.
pub fn provide_board_contexts(store: StoreSignal) {
    // Derive the live prompt's options and expose them so board entities can
    // route each option to itself and open a context menu.
    let pending = Signal::derive(move || store.with(interaction::pending_options));
    provide_context(PendingOptions(pending));

    // The option-less counterpart: an anchored `Confirm` (the Mythos encounter
    // draw) has no option to route, so the encounter deck reads the *request*
    // anchor instead (ADR 0011).
    let confirm_anchor = Signal::derive(move || store.with(interaction::confirm_anchor));
    provide_context(ConfirmAnchor(confirm_anchor));

    // "A decision modal is up" — the board's cards read it to keep their glow
    // while giving up their context menu, since the modal is a decision's sole
    // surface.
    let decision_live = Signal::derive(move || store.with(decision::modal_is_live));
    provide_context(DecisionLive(decision_live));

    // Multi-select (PickMultiple) selection state, shared by the hand cards and
    // the prompt banner; cleared whenever a PickMultiple isn't live.
    let selected = RwSignal::new(BTreeSet::<u32>::new());
    let multi_active = Signal::derive(move || store.with(interaction::is_multi_select));
    Effect::new(move |_| {
        if !multi_active.get() {
            selected.set(BTreeSet::new());
        }
    });
    provide_context(MultiSelect {
        active: multi_active,
        selected,
    });
}

/// The main column: the board and its in-game modals inside the lockout
/// wrapper, then the lock banner, then the picker and the terminal screens.
///
/// A component rather than inline tags so a headless test can mount the exact
/// composition the app does. The web tests' `mount_board` mounts it, so every
/// board test runs inside the real wrapper, and "`.action-bar` is absent from
/// the DOM" is asserted against the real composition rather than a copy of it.
///
/// The wrapper carries `inert` while [`ConnStatus::locks_board`], so the board
/// and its modals take no input until a `Hello` says the client is connected.
/// It is `display: contents` and creates no stacking context, so the modals
/// inside it stack against the screens outside it by z-index alone.
///
/// [`ConnStatus::locks_board`]: crate::store::ConnStatus::locks_board
#[component]
pub fn MainColumn() -> impl IntoView {
    let store = store::use_store();
    view! {
        <div class="main-column">
            <div class="board-lock" inert=move || store.with(|s| s.status.locks_board())>
                <BoardView/>
                <BoardOverlays/>
            </div>
            <LockBanner/>
            <Screens/>
        </div>
    }
}

/// The board's in-game modals: the skill-test result, the decision modal and
/// the prompt banner. Each is its own viewport-fixed overlay, and all of them
/// sit inside the lockout wrapper with the board.
#[component]
fn BoardOverlays() -> impl IntoView {
    view! {
        {
            #[cfg(target_arch = "wasm32")]
            { view! {
                <SkillTestResultView/>
                <DecisionView/>
                <PromptBanner/>
            }.into_any() }
            #[cfg(not(target_arch = "wasm32"))]
            { ().into_any() }
        }
    }
}

/// The banner that says why the board is locked. It sits outside the wrapper,
/// and renders only at a status with a
/// [`lock_banner`](crate::store::ConnStatus::lock_banner).
#[component]
fn LockBanner() -> impl IntoView {
    let store = store::use_store();
    move || {
        store
            .with(|s| s.status.lock_banner())
            .map(|text| view! { <div class="lock-banner">{text}</div> })
    }
}

/// The screens that take input while the board is locked: the pre-game
/// picker, the sign-in screen, the unresumable-game overlay and the
/// version-mismatch overlay. Each shows only at a status other than
/// `Connected`, so all of them sit outside the lockout wrapper.
///
/// The picker only renders pre-game. The sign-in screen covers the rest, since
/// nothing behind it answers while signed out. The unresumable overlay is
/// terminal and covers the board it can't resume. The version-mismatch overlay
/// is last, and stacks above everything: it is terminal, so nothing it covers
/// can still be acted on.
#[component]
fn Screens() -> impl IntoView {
    view! {
        {
            #[cfg(target_arch = "wasm32")]
            { view! {
                <PickerView/>
                <SignInView/>
                <UnresumableView/>
                <VersionMismatchView/>
            }.into_any() }
            #[cfg(not(target_arch = "wasm32"))]
            { ().into_any() }
        }
    }
}
