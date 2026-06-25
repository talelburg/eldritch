//! Eldritch web client entrypoint: install the panic hook and mount the app.

fn main() {
    console_error_panic_hook::set_once();
    // Install the card registry so `max_health()` / `max_sanity()` can resolve
    // investigator-card capacity during board rendering (#448).
    let _ = game_core::card_registry::install(cards::REGISTRY);
    leptos::mount::mount_to_body(web::app::App);
}
