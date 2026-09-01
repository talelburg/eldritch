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
    apply, attach_to_location, deal_damage_to_enemy, defeat_investigator, discard_random_from_hand,
    enemy_can_enter_location, legal_actions, location_id_by_code, modified_value,
    place_in_threat_area, put_set_aside_card_into_play, relocate_enemy,
    reshuffle_encounter_discard, resolve_choice_count, resolve_encounter_card, reveal_location,
    round_end_advance, round_end_advance_affordable, seat_and_open, shortest_first_steps,
    suspend_for_native_choice, take_damage, ApplyResult, ChoiceOption, ChoiceResolution,
    Contribution, ContributionSource, Cx, EngineOutcome, EvalContext, InputKind, InputRequest,
    ModifiedQuantity, ModifierBreakdown, ModifierTarget, OptionId, OptionTarget, PromptNature,
    ReadContext, ResumeToken, TurnAction,
};
pub use event::{Event, FailureReason, TraumaKind};
pub use rng::RngState;
pub use scenario::{
    LocationLayout, ResolutionId, ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry,
};
pub use state::{
    resolve_token, Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    DifficultyBasis, EliminationCause, Enemy, EnemyId, GameState, Investigator, InvestigatorId,
    Lifetime, Location, LocationId, Phase, RecordedModifier, SkillKind, SkillTestId, Skills,
    Status, TokenModifiers, TokenResolution, UseKind, Zone,
};
