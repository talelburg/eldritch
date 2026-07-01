//! The root Leptos component for the Eldritch web client.

use leptos::prelude::*;

use crate::board::BoardView;
use crate::store::provide_store;

#[component]
pub fn App() -> impl IntoView {
    provide_store();

    // Spawn the browser transport only on wasm; native/headless-reducer
    // builds render from a signal that tests drive directly.
    #[cfg(target_arch = "wasm32")]
    {
        let store = crate::store::use_store();
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
