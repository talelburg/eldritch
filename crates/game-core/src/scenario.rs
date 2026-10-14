//! Scenario-module data types: identifier, scenario ending, and the
//! static `ScenarioModule` / `ScenarioRegistry` pair that bridges
//! engine ↔ scenarios crate.
//!
//! Mirrors [`card_registry`](crate::card_registry)'s shape: the
//! `scenarios` crate (which depends on `game-core`) provides a static
//! [`ScenarioRegistry`] of function pointers, and the host hands it to
//! every [`apply`](crate::engine::apply) inside a
//! [`Registries`](crate::engine::Registries).
//! The engine watches `GameState.ending` for a `None`->`Some`
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
//! serializable. On reload, the host passes `REGISTRY` again and the
//! engine looks the module up by id — the action log replays
//! deterministically.

use std::fmt;

use card_dsl::dsl::TokenOutcome;
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
/// `end_scenario`.
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
    pub const fn number(self) -> u8 {
        self.0
    }
}

impl fmt::Display for ResolutionId {
    /// Renders as the campaign guide titles the ending: `Resolution 3`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

/// A scenario's **location layout**: the connections printed as connection
/// symbols on its location cards, as unordered pairs of card codes. Each
/// pair is wired bidirectionally when the second of its two locations
/// enters play.
///
/// The layout is scenario-owned rather than card-owned because the pinned
/// `ArkhamDB` snapshot has **no field for connections**. A field census over
/// all 1,257 locations in `data/arkhamdb-snapshot/` yields `shroud`,
/// `clues`, `back_link` (the a/b side pairing, not connections) and prose
/// in `back_text` (*"Easttown is connected to Velma's Diner."*), and
/// nothing else that names a neighbour. So no scenario's layout is
/// derivable from card metadata at this upstream, and the scenario module
/// is the smallest thing that knows the whole board.
pub type LocationLayout = &'static [(&'static str, &'static str)];

/// Static bundle of function pointers for one
/// scenario module.
///
/// Mirrors [`CardRegistry`](crate::card_registry::CardRegistry)'s
/// shape: no `dyn`, no `Box`, [`Copy`]-able.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioModule {
    /// Resolve a drawn chaos **symbol** token (Skull/Cultist/Tablet/
    /// `ElderThing`) against live board state, as the scenario reference
    /// card's [`TokenOutcome`] — the same shape an investigator's elder sign
    /// carries. `None` means this scenario has no reference-card symbol
    /// effects (test fixtures); the engine then falls back to the static
    /// [`TokenModifiers`](crate::state::TokenModifiers) table. Never called
    /// for Numeric/AutoFail/ElderSign tokens.
    ///
    /// The hook runs once, at token reveal (ST.3), so a board-gated branch
    /// (The Gathering's `[tablet]` damage) is decided up front. The returned
    /// `modifier` is recorded unevaluated and read at ST.5, so a hook that
    /// wants a live value returns a live [`IntExpr`](card_dsl::dsl::IntExpr);
    /// one that returns `IntExpr::Lit` freezes the value it computed at reveal.
    pub resolve_symbol: Option<fn(ChaosToken, &SymbolCtx) -> TokenOutcome>,
    /// Build the scenario's initial [`GameState`]. Places locations,
    /// populates encounter / act / agenda decks, sets chaos-bag
    /// modifiers, etc.
    pub setup: fn() -> GameState,
    /// Apply the resolution's effects (XP, trauma, scenario-end cleanup).
    /// Called by [`apply`](crate::engine::apply) exactly once, when the
    /// engine observes `GameState.ending` transition from `None` to
    /// `Some` during an apply. Receives the events buffer so changes are
    /// observable to clients.
    ///
    /// For the Phase-4 synthetic fixture this is a no-op. Phase 9 fills in
    /// real bodies once the campaign log lands.
    pub apply_resolution: fn(ScenarioEnding, &mut GameState, &mut Vec<Event>),
    /// This scenario's [`LocationLayout`]. Read when a set-aside location
    /// enters play, which is the only moment both of a connection's
    /// [`LocationId`]s exist. `&[]` for a scenario whose locations are all
    /// laid out by `setup()` (test fixtures, and any board with no
    /// deferred rooms).
    pub layout: LocationLayout,
}

/// Lookup table of [`ScenarioModule`]s, keyed by [`ScenarioId`].
///
/// The `scenarios` crate exposes a `pub const REGISTRY: ScenarioRegistry`
/// wrapping its own `by_id` lookup; hosts pass it to every
/// [`apply`](crate::engine::apply) inside a
/// [`Registries`](crate::engine::Registries).
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRegistry {
    /// Look up a scenario module by its id. Returns `None` for ids
    /// not known to this registry.
    pub module_for: fn(&ScenarioId) -> Option<&'static ScenarioModule>,
}

impl ScenarioRegistry {
    /// The knows-nothing registry: every id is unknown. `const`, so it can
    /// back a `&'static` [`Registries`](crate::engine::Registries) half for a
    /// host or test that runs no scenario module.
    pub const EMPTY: Self = Self {
        module_for: |_| None,
    };
}

/// The active scenario's [`LocationLayout`], or `&[]` when the apply runs
/// no scenario module (no active scenario, or an id the scenario registry
/// doesn't know). `scenario` is the module the apply resolved once from
/// `state.scenario_id`; mirrors [`resolve_symbol_token`].
///
/// An absent layout is not an error: a board with no set-aside locations
/// never asks, and a scenario that laid its whole board out in `setup()`
/// has already wired it.
#[must_use]
pub fn scenario_layout(scenario: Option<&ScenarioModule>) -> LocationLayout {
    scenario.map_or(&[], |module| module.layout)
}

/// Resolve a drawn chaos symbol token against the active scenario's
/// reference-card effects, if any, through
/// [`ScenarioModule::resolve_symbol`] on `scenario` — the module the apply
/// resolved once from `state.scenario_id`. Returns `None` when the apply runs
/// no scenario module, or the module has no symbol hook — callers then fall back to the static
/// [`TokenModifiers`](crate::state::TokenModifiers) path.
#[must_use]
pub fn resolve_symbol_token(
    scenario: Option<&ScenarioModule>,
    state: &GameState,
    token: ChaosToken,
    investigator: InvestigatorId,
) -> Option<TokenOutcome> {
    let hook = scenario?.resolve_symbol?;
    Some(hook(token, &SymbolCtx::new(state, investigator)))
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::state::GameStateBuilder;

    fn empty_state() -> GameState {
        GameStateBuilder::new().build()
    }

    fn no_op_apply(_ending: ScenarioEnding, _state: &mut GameState, _events: &mut Vec<Event>) {}

    static FAKE_MODULE: ScenarioModule = ScenarioModule {
        resolve_symbol: None,
        setup: empty_state,
        apply_resolution: no_op_apply,
        layout: &[],
    };

    fn fake_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "fake" {
            Some(&FAKE_MODULE)
        } else {
            None
        }
    }

    fn fake_registry() -> ScenarioRegistry {
        ScenarioRegistry {
            module_for: fake_module_for,
        }
    }

    #[test]
    fn module_for_returns_known_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("fake");
        let module = (reg.module_for)(&id).expect("known id should resolve");
        assert!(ptr::eq(module, ptr::addr_of!(FAKE_MODULE)));
    }

    #[test]
    fn module_for_returns_none_for_unknown_id() {
        let reg = fake_registry();
        let id = ScenarioId::new("nonexistent");
        assert!((reg.module_for)(&id).is_none());
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
