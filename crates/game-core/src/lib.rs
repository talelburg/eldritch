//! Eldritch rules engine.
//!
//! This crate is the heart of the simulator. It owns the game state, action
//! and event types, the apply loop, and the effect system. It has no I/O and
//! no async; everything here is pure and deterministic, so the same code
//! compiles to native (server) and `wasm32` (client).
