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
use crate::state::{ChaosToken, GameState, InvestigatorId, LocationId};

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

/// Read-only board view handed to a scenario's symbol-token hook
/// ([`ScenarioModule::resolve_symbol`]). Carries the testing investigator
/// and the live state so the hook can compute board-dependent values
/// (e.g. "number of Ghoul enemies at your location").
pub struct SymbolCtx<'a> {
    state: &'a GameState,
    investigator: InvestigatorId,
}

impl<'a> SymbolCtx<'a> {
    /// Construct a context for `investigator` over `state`.
    #[must_use]
    pub fn new(state: &'a GameState, investigator: InvestigatorId) -> Self {
        Self {
            state,
            investigator,
        }
    }

    /// The full game state (read-only).
    #[must_use]
    pub fn state(&self) -> &GameState {
        self.state
    }

    /// The investigator whose skill test drew the symbol.
    #[must_use]
    pub fn investigator(&self) -> InvestigatorId {
        self.investigator
    }

    /// The testing investigator's current location, if placed.
    #[must_use]
    pub fn investigator_location(&self) -> Option<LocationId> {
        self.state
            .investigators
            .get(&self.investigator)
            .and_then(|inv| inv.current_location)
    }
}

/// What a drawn chaos **symbol** token does this skill test: a numeric
/// modifier plus side effects, split by resolution timing.
///
/// The `modifier` is applied to the skill total *before* success/failure
/// is computed; `immediate` effects apply regardless of outcome (e.g.
/// 01104 tablet's board-gated damage); `on_fail` effects apply only when
/// the test fails (e.g. 01104 cultist's horror). The hook is evaluated
/// once at token reveal, so board-gated branches are decided up front.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolOutcome {
    /// Added to the test's skill total.
    pub modifier: i8,
    /// Applied to the testing investigator regardless of pass/fail.
    pub immediate: Vec<TokenEffect>,
    /// Applied to the testing investigator only if the test fails.
    pub on_fail: Vec<TokenEffect>,
}

/// A symbol token's side effect on the testing investigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenEffect {
    /// Deal N damage to the testing investigator.
    Damage(u8),
    /// Deal N horror to the testing investigator.
    Horror(u8),
}

/// Static, host-installed bundle of function pointers for one
/// scenario module.
///
/// Mirrors [`CardRegistry`](crate::card_registry::CardRegistry)'s
/// shape: no `dyn`, no `Box`, [`Copy`]-able.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Resolve a drawn chaos **symbol** token (Skull/Cultist/Tablet/
    /// `ElderThing`) against live board state. `None` means this scenario
    /// has no reference-card symbol effects (test fixtures); the engine
    /// then falls back to the static [`TokenModifiers`](crate::state::TokenModifiers)
    /// table. Never called for Numeric/AutoFail/ElderSign tokens.
    pub resolve_symbol: Option<fn(ChaosToken, &SymbolCtx) -> SymbolOutcome>,
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

/// Resolve a drawn chaos symbol token against the active scenario's
/// reference-card effects, if any. Routes
/// `state.scenario_id` → installed scenario registry → `module_for` →
/// [`ScenarioModule::resolve_symbol`]. Returns `None` when there is no
/// active scenario, no registry, an unknown id, or the module has no
/// symbol hook — callers then fall back to the static
/// [`TokenModifiers`](crate::state::TokenModifiers) path.
#[must_use]
pub fn resolve_symbol_token(
    state: &GameState,
    token: crate::state::ChaosToken,
    investigator: InvestigatorId,
) -> Option<SymbolOutcome> {
    let id = state.scenario_id.as_ref()?;
    let registry = crate::scenario_registry::current()?;
    let module = (registry.module_for)(id)?;
    let hook = module.resolve_symbol?;
    Some(hook(token, &SymbolCtx::new(state, investigator)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn symbol_outcome_default_is_inert() {
        let out = SymbolOutcome::default();
        assert_eq!(out.modifier, 0);
        assert!(out.immediate.is_empty());
        assert!(out.on_fail.is_empty());
    }

    #[test]
    fn token_effect_variants_construct() {
        assert_eq!(TokenEffect::Damage(1), TokenEffect::Damage(1));
        assert_ne!(TokenEffect::Damage(1), TokenEffect::Horror(1));
    }

    #[test]
    fn symbol_ctx_exposes_investigator_and_state() {
        let state = GameStateBuilder::new().build();
        let inv = InvestigatorId(1);
        let ctx = SymbolCtx::new(&state, inv);
        assert_eq!(ctx.investigator(), inv);
        // No investigator placed → location is None.
        assert!(ctx.investigator_location().is_none());
    }
}
