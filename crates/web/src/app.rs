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
                { view! { <crate::input::AwaitingInputView/><DebugSubmit/> }.into_any() }
                #[cfg(not(target_arch = "wasm32"))]
                { ().into_any() }
            }
        </main>
    }
}

/// Wasm-only debug control: pushes a `ClientMessage::Submit { EndTurn }`
/// onto the transport's outbound channel, exercising the send path
/// end-to-end. P6.7 builds the real action controls on this seam.
#[cfg(target_arch = "wasm32")]
#[component]
fn DebugSubmit() -> impl IntoView {
    let tx = use_context::<crate::transport::OutboundTx>()
        .expect("OutboundTx provided by transport::start");
    let on_click = move |_| {
        let _ = tx.unbounded_send(protocol::ClientMessage::Submit {
            action: game_core::PlayerAction::EndTurn,
        });
    };
    view! { <button on:click=on_click>"Submit EndTurn (debug)"</button> }
}
