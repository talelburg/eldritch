//! Eldritch web client entrypoint: install the panic hook and mount the app.

use cards::REGISTRY;
use game_core::card_registry;
use leptos::mount;
use web::app::App;

fn main() {
    console_error_panic_hook::set_once();
    // Install the card registry so `max_health()` / `max_sanity()` can resolve
    // investigator-card capacity during board rendering (#448).
    let _ = card_registry::install(REGISTRY);
    mount::mount_to_body(App);
}
