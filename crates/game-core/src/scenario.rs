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
    /// `ArkhamDB` card code of this scenario's single reference card —
    /// the card whose chaos **symbol** abilities (skull / cultist /
    /// tablet / elder-thing) are printed on it (e.g. `"01104"` for The
    /// Gathering). Plain data: ownership of the symbol effect stays on
    /// the card, but access flows through the scenario module.
    ///
    /// `&'static str` (not [`CardCode`](crate::state::CardCode)) so the
    /// struct stays [`Copy`] and const-constructible in `static` /
    /// `const` module literals, matching the `CODE: &str` convention
    /// card impls already use. Empty string means the scenario has no
    /// reference card (test fixtures / synthetic modules).
    ///
    /// Slice 1 B1 only *routes* to this code (see
    /// [`active_reference_card`]); evaluating the symbol ability against
    /// board state lands in Group C with the `01104` impl.
    pub reference_card: &'static str,
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

/// The active scenario's reference-card code, or `None`.
///
/// Routes `state.scenario_id` → the installed scenario registry →
/// `module_for` → [`ScenarioModule::reference_card`]. Returns `None`
/// when there is no active scenario, no registry is installed, or the
/// id is unknown — the same tolerant shape as
/// [`apply`](crate::engine::apply)'s resolution lookup.
///
/// The returned code may be the empty string for fixture/synthetic
/// modules with no symbol content; callers that evaluate symbol
/// abilities (Group C) treat `""` as "no reference card".
#[must_use]
pub fn active_reference_card(state: &GameState) -> Option<&'static str> {
    reference_card_with_registry(state, crate::scenario_registry::current())
}

/// Registry-parameterized core of [`active_reference_card`], split out so
/// tests can pass an explicit [`ScenarioRegistry`] instead of relying on
/// the process-global `OnceLock`.
fn reference_card_with_registry(
    state: &GameState,
    registry: Option<&ScenarioRegistry>,
) -> Option<&'static str> {
    let id = state.scenario_id.as_ref()?;
    let module = (registry?.module_for)(id)?;
    Some(module.reference_card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GameState;
    use crate::test_support::GameStateBuilder;

    fn dummy_setup() -> GameState {
        GameStateBuilder::new().build()
    }
    fn dummy_resolution(_: &Resolution, _: &mut GameState, _: &mut Vec<Event>) {}

    static GATHERING_MODULE: ScenarioModule = ScenarioModule {
        reference_card: "01104",
        setup: dummy_setup,
        apply_resolution: dummy_resolution,
    };

    fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        (id.as_str() == "the-gathering").then_some(&GATHERING_MODULE)
    }

    fn registry() -> ScenarioRegistry {
        ScenarioRegistry { module_for }
    }

    #[test]
    fn returns_reference_card_for_active_scenario() {
        let state = GameStateBuilder::new()
            .with_scenario_id(ScenarioId::new("the-gathering"))
            .build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            Some("01104"),
        );
    }

    #[test]
    fn returns_none_when_no_scenario_id() {
        let state = GameStateBuilder::new().build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            None,
        );
    }

    #[test]
    fn returns_none_when_no_registry_installed() {
        let state = GameStateBuilder::new()
            .with_scenario_id(ScenarioId::new("the-gathering"))
            .build();
        assert_eq!(reference_card_with_registry(&state, None), None);
    }

    #[test]
    fn returns_none_for_unknown_scenario() {
        let state = GameStateBuilder::new()
            .with_scenario_id(ScenarioId::new("nonexistent"))
            .build();
        assert_eq!(
            reference_card_with_registry(&state, Some(&registry())),
            None,
        );
    }
}
