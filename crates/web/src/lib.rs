//! Eldritch web client library: Leptos CSR components, compiled to WASM.
//!
//! The binary (`main.rs`) is a thin entrypoint that mounts [`app::App`].
//! Components live here so integration tests in `tests/` — and later Phase-6
//! components — can import them.

pub mod app;
pub mod board;
pub mod store;
pub mod url;

#[cfg(target_arch = "wasm32")]
pub mod transport;

#[cfg(target_arch = "wasm32")]
pub mod input;

#[cfg(target_arch = "wasm32")]
pub mod picker;
