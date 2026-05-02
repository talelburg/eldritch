//! Eldritch web client. Phase 0 placeholder — renders a single greeting.

use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"Eldritch"</h1>
            <p>"Coming soon — a digital simulator for the Arkham Horror LCG."</p>
        </main>
    }
}
