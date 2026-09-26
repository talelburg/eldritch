//! Eldritch rules engine.
//!
//! This crate is the heart of the simulator. It owns the game state, action
//! and event types, the apply loop, and the effect system. It has no I/O and
//! no async; everything here is pure and deterministic, so the same code
//! compiles to native (server) and `wasm32` (client).
//!
//! # Layout
//!
//! - [`state`] — pure data: [`GameState`](state::GameState) and the entities it contains.
//! - [`action`] — the [`Action`](action::Action) enum (the alphabet of the action log),
//!   split into [`PlayerAction`](action::PlayerAction) (human input) and [`EngineRecord`](action::EngineRecord)
//!   (engine-recorded RNG and system events).
//! - [`event`] — the [`Event`](event::Event) enum (state-change records emitted by the
//!   engine as actions resolve).
//! - [`engine`] — the [`apply`](engine::apply) loop and
//!   [`EngineOutcome`](engine::EngineOutcome) terminal status.
//!
//! Subsequent PRs add the RNG, phase machine, and test harness.

pub mod action;
pub mod card_registry;
pub mod engine;
pub mod event;
pub mod rng;
pub mod scenario;
pub mod scenario_registry;
pub mod state;

pub mod test_support;
