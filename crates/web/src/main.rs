//! Eldritch web client entrypoint: install the panic hook and mount the app.

use leptos::prelude::*;
use web::app::App;
use web::registry;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        // The card registry every render path reads: card faces, names, and
        // investigator capacity (`max_health` / `max_sanity`, #448).
        registry::provide_cards(&cards::REGISTRY);
        view! { <App/> }
    });
}
