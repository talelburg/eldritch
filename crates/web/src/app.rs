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
            <h1>"Eldritch"</h1>
            <BoardView/>
            {
                #[cfg(target_arch = "wasm32")]
                { view! { <crate::input::AwaitingInputView/><crate::controls::ActionControls/> }.into_any() }
                #[cfg(not(target_arch = "wasm32"))]
                { ().into_any() }
            }
        </main>
    }
}
