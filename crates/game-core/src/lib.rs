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
//! - [`engine`] — the [`apply`] loop and [`EngineOutcome`] terminal status.
//!
//! Subsequent PRs add the RNG, phase machine, and test harness.

pub mod action;
pub mod card_data;
pub mod card_registry;
pub mod dsl;
pub mod engine;
pub mod event;
pub mod rng;
pub mod state;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use action::{Action, EngineRecord, InputResponse, PlayerAction};
pub use card_data::{CardMetadata, CardType, Class, SkillIcons, Slot};
pub use card_registry::CardRegistry;
pub use engine::{apply, ApplyResult, EngineOutcome, InputRequest, ResumeToken};
pub use event::{Event, FailureReason};
pub use rng::RngState;
pub use state::{
    resolve_token, CardCode, ChaosBag, ChaosToken, DefeatCause, Enemy, EnemyId, GameState,
    Investigator, InvestigatorId, Location, LocationId, Phase, SkillKind, Skills, Status,
    TokenModifiers, TokenResolution, Zone,
};
