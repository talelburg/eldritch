//! Eldritch web client entrypoint: install the panic hook and mount the app.

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(web::app::App);
}
