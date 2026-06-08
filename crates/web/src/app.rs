//! The root Leptos component for the Eldritch web client.

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"Eldritch"</h1>
            <p>"Coming soon — a digital simulator for the Arkham Horror LCG."</p>
        </main>
    }
}
