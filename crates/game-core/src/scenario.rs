//! Scenario-module data types: identifier, scenario ending, and the
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

/// A printed resolution point, `(→R#)`.
///
/// Rules Reference, `Winning and Losing`: *"Some instructions in the act
/// deck (as well as on other encounter cardtypes) contain resolution
/// points, in the format of: '**(→R#)**.'"* The `#` is a number — the
/// campaign guide's endings are titled "Resolution 1", "Resolution 2",
/// and so on — so this is a `u8`, not a string. That keeps
/// [`ScenarioEnding`] [`Copy`], which is why the act/agenda dispatch
/// sites can read a terminal card's resolution point without cloning it
/// out from under the `&mut GameState` they are about to hand to
/// `request_resolution`.
///
/// The meaning of a given number is scenario-local: the campaign guide's
/// "do not read until end of game" section is what interprets it, and
/// Phase 9's campaign log looks the ending up by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResolutionId(u8);

impl ResolutionId {
    /// Construct a resolution point from its printed number.
    #[must_use]
    pub const fn new(n: u8) -> Self {
        Self(n)
    }

    /// The printed number, i.e. the `#` in `(→R#)`.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for ResolutionId {
    /// Renders as the campaign guide titles the ending: `Resolution 3`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Resolution {}", self.0)
    }
}

/// How a scenario ended.
///
/// A scenario ends at a **resolution point**, or with **none reached** —
/// not at a win or a loss. Rules Reference, `Winning and Losing`, gives
/// the two doors: the act deck and the agenda deck both invoke resolution
/// points in the same `(→R#)` notation, and *"Should the scenario end with
/// no resolution being reached (for example, if all investigators have been
/// eliminated or have resigned), instructions for resolving the scenario
/// can be found in the 'do not read until end of game' section of the
/// campaign guide."*
///
/// Win and loss are **not** stored here. They are a standalone-mode
/// projection over this fact: in campaign play *"players will proceed to
/// the next scenario in the campaign regardless of the outcome"*, and only
/// the standalone bullet collapses the endings into two. See
/// `docs/adr/0012-a-scenario-ends-at-a-resolution-point-or-at-none.md`.
///
/// `#[non_exhaustive]` reserves room for Phase 9's campaign-log work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScenarioEnding {
    /// A printed resolution point was reached, from the act deck or the
    /// agenda deck. Which deck invoked it is deliberately not recorded:
    /// resolution points also appear *"on other encounter cardtypes"*, so
    /// the deck is a proxy rather than a fact, and the only rule that asks
    /// (standalone mode's *"they win if they complete a resolution on an
    /// act card"*) is scenario-local knowledge the campaign guide owns.
    Resolution(ResolutionId),
    /// The scenario ended with no resolution point reached — Rules
    /// Reference `Elimination` step 6, *"If there are no remaining
    /// players, the scenario ends."* The campaign guide gives this ending
    /// its own untitled entry, *"If no resolution was reached (each
    /// investigator resigned or was defeated)"*.
    NoResolution,
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
    pub(crate) fn new(state: &'a GameState, investigator: InvestigatorId) -> Self {
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
    pub apply_resolution: fn(ScenarioEnding, &mut GameState, &mut Vec<Event>),
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
