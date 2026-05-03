//! Eldritch rules engine.
//!
//! This crate is the heart of the simulator. It owns the game state, action
//! and event types, the apply loop, and the effect system. It has no I/O and
//! no async; everything here is pure and deterministic, so the same code
//! compiles to native (server) and `wasm32` (client).
//!
//! # Layout
//!
//! - [`state`] — pure data: [`GameState`] and the entities it contains.
//! - [`action`] — the [`Action`] enum (the alphabet of the action log),
//!   split into [`PlayerAction`] (human input) and [`EngineRecord`]
//!   (engine-recorded RNG and system events).
//! - [`event`] — the [`Event`] enum (state-change records emitted by the
//!   engine as actions resolve).
//!
//! Subsequent PRs add the apply loop, RNG, phase machine, and test harness.

pub mod action;
pub mod event;
pub mod state;

pub use action::{Action, EngineRecord, InputResponse, PlayerAction};
pub use event::Event;
pub use state::{
    ChaosBag, ChaosToken, GameState, Investigator, InvestigatorId, Location, LocationId, Phase,
    Skills,
};
