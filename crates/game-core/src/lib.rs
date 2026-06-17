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
pub mod card_registry;
pub mod engine;
pub mod event;
pub mod rng;
pub mod scenario;
pub mod scenario_registry;
pub mod state;

pub mod test_support;

/// Re-exports of the [`card_dsl::card_data`] module, kept under the
/// historical `game_core::card_data` path so downstream code that
/// imports via `game_core::card_data::*` keeps compiling. The
/// definitions themselves live in [`card_dsl`].
pub use card_dsl::card_data;
/// Re-exports of the [`card_dsl::dsl`] module, kept under the
/// historical `game_core::dsl` path so downstream code that imports
/// via `game_core::dsl::*` keeps compiling. The definitions themselves
/// live in [`card_dsl`].
pub use card_dsl::dsl;

pub use action::{Action, EngineRecord, InputResponse, PlayerAction};
pub use card_data::{CardMetadata, CardType, Class, SkillIcons, Slot};
pub use card_registry::CardRegistry;
pub use engine::{
    apply, attach_to_location, deal_damage_to_enemy, effective_shroud, location_id_by_code,
    place_doom_on_current_agenda, place_in_threat_area, reshuffle_encounter_discard,
    resolve_encounter_card, reveal_location, shortest_first_steps, spawn_set_aside_enemy,
    take_damage, ApplyResult, ChoiceOption, Cx, EngineOutcome, EvalContext, InputRequest, OptionId,
    ResumeToken,
};
pub use event::{Event, FailureReason, TraumaKind};
pub use rng::RngState;
pub use scenario::{Resolution, ScenarioId, ScenarioModule, ScenarioRegistry};
pub use state::{
    resolve_token, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    DefeatCause, Enemy, EnemyId, GameState, Investigator, InvestigatorId, Location, LocationId,
    PendingSkillModifier, Phase, SkillKind, Skills, Status, TokenModifiers, TokenResolution,
    UseKind, Zone,
};
