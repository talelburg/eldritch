//! The root Leptos component for the Eldritch web client.

use leptos::prelude::*;

use crate::board::BoardView;
use crate::store::provide_store;

#[component]
pub fn App() -> impl IntoView {
    let store = provide_store();

    // Derive the live prompt's options and expose them so board entities can
    // route each option to itself and open a context menu (#536).
    let pending = Signal::derive(move || store.with(crate::interaction::pending_options));
    provide_context(crate::interaction::PendingOptions(pending));

    // The option-less counterpart: an anchored `Confirm` (the Mythos encounter
    // draw) has no option to route, so the encounter deck reads the *request*
    // anchor instead (ADR 0011).
    let confirm_anchor = Signal::derive(move || store.with(crate::interaction::confirm_anchor));
    provide_context(crate::interaction::ConfirmAnchor(confirm_anchor));

    // Multi-select (PickMultiple) selection state, shared by the hand cards and
    // the prompt banner; cleared whenever a PickMultiple isn't live (#538).
    let selected = RwSignal::new(std::collections::BTreeSet::<u32>::new());
    let multi_active = Signal::derive(move || store.with(crate::interaction::is_multi_select));
    Effect::new(move |_| {
        if !multi_active.get() {
            selected.set(std::collections::BTreeSet::new());
        }
    });
    provide_context(crate::interaction::MultiSelect {
        active: multi_active,
        selected,
    });

    // Spawn the browser transport only on wasm; native/headless-reducer
    // builds render from a signal that tests drive directly.
    #[cfg(target_arch = "wasm32")]
    {
        crate::transport::start(store);
    }

    view! {
        <main>
            <header class="app-header">
                <h1>"Eldritch"</h1>
                <crate::status_bar::StatusBarView/>
            </header>
            <div class="layout">
                <crate::event_log::EventLogView/>
                <div class="main-column">
                    <BoardView/>
                    <Overlays/>
                </div>
                <crate::turn_tracker::TurnTrackerView/>
            </div>
        </main>
    }
}

/// Everything the app layers over the board: the pre-game picker, the skill-test
/// result modal, the prompt banner and the version-mismatch overlay.
///
/// A component rather than three inline tags so a headless test can mount the
/// exact set the app does — which is how "`.action-bar` is absent from the DOM"
/// is asserted against the real composition rather than against a copy of it
/// (#541). The sticky bar that used to hold these is gone; each of the three is
/// now its own viewport-fixed overlay, and the picker only renders pre-game.
/// The version-mismatch overlay is last, and stacks above the rest: it is
/// terminal, so nothing it covers can still be acted on (#770).
#[component]
pub fn Overlays() -> impl IntoView {
    view! {
        {
            #[cfg(target_arch = "wasm32")]
            { view! {
                <crate::picker::PickerView/>
                <crate::skill_test_result::SkillTestResultView/>
                <crate::prompt_banner::PromptBanner/>
                <crate::version_mismatch::VersionMismatchView/>
            }.into_any() }
            #[cfg(not(target_arch = "wasm32"))]
            { ().into_any() }
        }
    }
}
