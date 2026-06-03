//! Scenario-module data types: identifier, resolution outcome, and the
//! static `ScenarioModule` / `ScenarioRegistry` pair that bridges
//! engine ↔ scenarios crate.
//!
//! Mirrors [`card_registry`](crate::card_registry)'s shape: the
//! `scenarios` crate (which depends on `game-core`) provides a static
//! [`ScenarioRegistry`] of function pointers, and the host installs it
//! once at startup via
//! [`scenario_registry::install`](crate::scenario_registry::install).
//! The engine watches `GameState.resolution` for a `None`->`Some`
//! transition during an apply (a push-model latch set at discrete
//! trigger sites); on that transition it looks up the active
//! scenario's module and runs its `apply_resolution`.
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! Same reasoning as `CardRegistry`: the surface is small and fixed.
//! Function pointers keep the registry [`Copy`], avoid vtable
//! overhead, and stay `serde`-free at the boundary. Tests construct
//! ad-hoc `ScenarioModule` values with mock function pointers.
//!
//! # Replay safety
//!
//! The active scenario id on `GameState` is
//! a serializable [`ScenarioId`]; function pointers are not
//! serializable. On reload, the host re-installs `REGISTRY` and the
//! engine looks the module up by id — the action log replays
//! deterministically.

use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::state::GameState;

/// Stable, serializable identifier for a scenario module.
///
/// Newtype around [`String`], mirroring
/// [`CardCode`](crate::state::CardCode). Kept on
/// [`GameState`] so action-log replay can
/// resolve the active scenario module via the registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioId(String);

impl ScenarioId {
    /// Construct a [`ScenarioId`] from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Outcome of a scenario.
///
/// Phase-4 minimal shape. Phase-9 will refine the payloads when the
/// typed campaign-log `Fact` enum and branching scenario sequencing
/// land; the `#[non_exhaustive]` annotation reserves that room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Resolution {
    /// Scenario completed successfully.
    Won {
        /// Per-scenario resolution branch identifier (e.g. `"R1"`,
        /// `"R2"`). The meaning is scenario-local — Phase 9's
        /// `next_scenario` orchestration interprets it.
        id: String,
    },
    /// Scenario ended in defeat.
    Lost {
        /// Human-readable cause for diagnostics. Not semantically
        /// load-bearing today; Phase 9 may swap for a typed enum.
        reason: String,
    },
}

/// Static, host-installed bundle of function pointers for one
/// scenario module.
///
/// Mirrors [`CardRegistry`](crate::card_registry::CardRegistry)'s
/// shape: no `dyn`, no `Box`, [`Copy`]-able.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Build the scenario's initial [`GameState`]. Places locations,
    /// populates encounter / act / agenda decks, sets chaos-bag
    /// modifiers, etc.
    pub setup: fn() -> GameState,
    /// Apply the resolution's effects (XP, trauma, scenario-end cleanup).
    /// Called by [`apply`](crate::engine::apply) exactly once, when the
    /// engine observes `GameState.resolution` transition from `None` to
    /// `Some` during an apply. Receives the events buffer so changes are
    /// observable to clients.
    ///
    /// For the Phase-4 synthetic fixture this is a no-op. Phase 9 fills in
    /// real bodies once the campaign log lands.
    pub apply_resolution: fn(&Resolution, &mut GameState, &mut Vec<Event>),
}

/// Lookup table of [`ScenarioModule`]s, keyed by [`ScenarioId`].
///
/// The `scenarios` crate exposes a `pub const REGISTRY: ScenarioRegistry`
/// wrapping its own `by_id` lookup; hosts install it once at startup
/// via
/// [`scenario_registry::install`](crate::scenario_registry::install).
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRegistry {
    /// Look up a scenario module by its id. Returns `None` for ids
    /// not known to this registry.
    pub module_for: fn(&ScenarioId) -> Option<&'static ScenarioModule>,
}
