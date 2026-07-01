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
                    {
                        #[cfg(target_arch = "wasm32")]
                        { view! {
                            // Sticky action bar: pinned to the viewport bottom so the
                            // controls stay reachable however far the (tall) board is
                            // scrolled. Invisible when nothing is pending.
                            <div class="action-bar">
                                <crate::picker::PickerView/>
                                <crate::skill_test_result::SkillTestResultView/>
                                <crate::input::AwaitingInputView/>
                            </div>
                            <crate::prompt_banner::PromptBanner/>
                        }.into_any() }
                        #[cfg(not(target_arch = "wasm32"))]
                        { ().into_any() }
                    }
                </div>
                <crate::turn_tracker::TurnTrackerView/>
            </div>
        </main>
    }
}
