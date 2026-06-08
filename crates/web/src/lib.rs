//! Eldritch web client library: Leptos CSR components, compiled to WASM.
//!
//! The binary (`main.rs`) is a thin entrypoint that mounts [`app::App`].
//! Components live here so integration tests in `tests/` — and later Phase-6
//! components — can import them.

pub mod app;
