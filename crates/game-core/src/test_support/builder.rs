//! Fluent [`TestGame`] builder.
//!
//! The single most important piece of test infrastructure in the
//! engine. Every card, scenario, and engine test that follows depends
//! on this — if the API is awkward, every test pays the cost. Keep it
//! ergonomic.
//!
//! # Example
//!
//! ```
//! use game_core::{
//!     apply, Action, PlayerAction, Phase, InvestigatorId,
//!     test_support::{test_investigator, test_location, TestGame},
//! };
//!
//! let state = TestGame::new()
//!     .with_phase(Phase::Investigation)
//!     .with_investigator(test_investigator(1))
//!     .with_location(test_location(10, "Study"))
//!     .with_active_investigator(InvestigatorId(1))
//!     .build();
//!
//! let result = apply(state, Action::Player(PlayerAction::EndTurn));
//! ```

use std::collections::{BTreeMap, VecDeque};

use crate::rng::RngState;
use crate::scenario::ScenarioId;
use crate::state::{
    ChaosBag, Enemy, EnemyId, FastActorScope, GameState, Investigator, InvestigatorId, Location,
    LocationId, OpenWindow, Phase, TokenModifiers, WindowKind,
};

/// Fluent builder for a [`GameState`].
///
/// Construct with [`TestGame::new`], chain `.with_*` setters and
/// adders, then call [`build`](TestGame::build) to get a `GameState`
/// ready for [`apply`](crate::apply).
#[derive(Debug, Clone)]
#[must_use = "TestGame is a builder; call .build() to produce a GameState"]
pub struct TestGame {
    investigators: BTreeMap<InvestigatorId, Investigator>,
    locations: BTreeMap<crate::state::LocationId, Location>,
    enemies: BTreeMap<EnemyId, Enemy>,
    chaos_bag: ChaosBag,
    token_modifiers: TokenModifiers,
    phase: Phase,
    round: u32,
    active_investigator: Option<InvestigatorId>,
    turn_order: Vec<InvestigatorId>,
    rng: RngState,
    mulligan_window: bool,
    open_windows: Vec<OpenWindow>,
    scenario_id: Option<ScenarioId>,
}

impl TestGame {
    /// Start a new builder with empty investigators / locations / chaos
    /// bag, `Phase::Mythos`, round 0, no active investigator, RNG
    /// seeded at zero. Most tests override the seed explicitly via
    /// [`with_rng_seed`](Self::with_rng_seed).
    pub fn new() -> Self {
        Self {
            investigators: BTreeMap::new(),
            locations: BTreeMap::new(),
            enemies: BTreeMap::new(),
            chaos_bag: ChaosBag::new([]),
            token_modifiers: TokenModifiers::default(),
            phase: Phase::Mythos,
            round: 0,
            active_investigator: None,
            turn_order: Vec::new(),
            rng: RngState::new(0),
            mulligan_window: false,
            open_windows: Vec::new(),
            scenario_id: None,
        }
    }

    /// Add an investigator to the state. Replaces any existing entry
    /// with the same id.
    pub fn with_investigator(mut self, investigator: Investigator) -> Self {
        self.investigators.insert(investigator.id, investigator);
        self
    }

    /// Add an investigator placed at `location`. Sets the investigator's
    /// `current_location` to `Some(location)` then inserts; equivalent to
    /// the pre-existing two-step `let mut inv = …; inv.current_location =
    /// Some(loc); .with_investigator(inv)` shape. Replaces any existing
    /// investigator entry with the same id, like [`with_investigator`].
    /// The named location itself must still be added separately via
    /// [`with_location`] (this helper does not insert one — most tests
    /// already have a fixture builder for the location with its specific
    /// shroud / connections / clues).
    ///
    /// # Example
    ///
    /// ```
    /// use game_core::{
    ///     InvestigatorId, LocationId,
    ///     test_support::{test_investigator, test_location, TestGame},
    /// };
    ///
    /// let state = TestGame::new()
    ///     .with_investigator_at(test_investigator(1), LocationId(10))
    ///     .with_location(test_location(10, "Study"))
    ///     .build();
    /// assert_eq!(
    ///     state.investigators[&InvestigatorId(1)].current_location,
    ///     Some(LocationId(10)),
    /// );
    /// ```
    ///
    /// [`with_investigator`]: Self::with_investigator
    /// [`with_location`]: Self::with_location
    pub fn with_investigator_at(
        mut self,
        mut investigator: Investigator,
        location: LocationId,
    ) -> Self {
        investigator.current_location = Some(location);
        self.investigators.insert(investigator.id, investigator);
        self
    }

    /// Add a location to the state. Replaces any existing entry with
    /// the same id.
    pub fn with_location(mut self, location: Location) -> Self {
        self.locations.insert(location.id, location);
        self
    }

    /// Add an enemy to the state. Replaces any existing entry with the
    /// same id.
    pub fn with_enemy(mut self, enemy: Enemy) -> Self {
        self.enemies.insert(enemy.id, enemy);
        self
    }

    /// Set the chaos bag. Replaces any prior bag.
    pub fn with_chaos_bag(mut self, chaos_bag: ChaosBag) -> Self {
        self.chaos_bag = chaos_bag;
        self
    }

    /// Set the per-scenario symbol-token modifiers. Defaults to all
    /// zeros if not called.
    pub fn with_token_modifiers(mut self, modifiers: TokenModifiers) -> Self {
        self.token_modifiers = modifiers;
        self
    }

    /// Set the current phase.
    pub fn with_phase(mut self, phase: Phase) -> Self {
        self.phase = phase;
        self
    }

    /// Set the round counter.
    pub fn with_round(mut self, round: u32) -> Self {
        self.round = round;
        self
    }

    /// Mark an investigator as the active one. The id must refer to an
    /// investigator already added via [`with_investigator`]; this is
    /// not enforced at build time but later [`apply`](crate::apply)
    /// calls will surface state-corruption invariant violations
    /// loudly.
    ///
    /// [`with_investigator`]: Self::with_investigator
    pub fn with_active_investigator(mut self, id: InvestigatorId) -> Self {
        self.active_investigator = Some(id);
        self
    }

    /// Set the turn order (lead-investigator-decided sequence in which
    /// investigators take their turns during the Investigation phase).
    pub fn with_turn_order(mut self, order: impl IntoIterator<Item = InvestigatorId>) -> Self {
        self.turn_order = order.into_iter().collect();
        self
    }

    /// Seed the deterministic RNG. Resets `draws` to 0.
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng = RngState::new(seed);
        self
    }

    /// Set the full RNG state (seed + draws). Useful for tests that
    /// want to start mid-stream.
    pub fn with_rng(mut self, rng: RngState) -> Self {
        self.rng = rng;
        self
    }

    /// Open the mulligan window. By default the window is closed so
    /// tests don't accidentally exercise Mulligan paths; opt in by
    /// calling this on the builder when a test wants to fire the
    /// Mulligan action directly without going through
    /// `StartScenario`.
    pub fn with_mulligan_window_open(mut self) -> Self {
        self.mulligan_window = true;
        self
    }

    /// Push an [`OpenWindow`] onto the build's `open_windows` stack
    /// for tests that need a specific window-state shape.
    ///
    /// The pushed window has no pending triggers (test paths that
    /// also need a reaction queue should manipulate `state` after
    /// `build()` rather than complicate this builder).
    pub fn with_open_window(mut self, kind: WindowKind, fast_actors: FastActorScope) -> Self {
        self.open_windows.push(OpenWindow {
            kind,
            pending_triggers: Vec::new(),
            fast_actors,
        });
        self
    }

    /// Set the scenario id this state belongs to. `None` (the
    /// default from [`TestGame::new`]) means the engine's post-apply
    /// resolution hook will short-circuit. Passing a `ScenarioId`
    /// means a `ScenarioRegistry` capable of resolving it must be
    /// installed *if you want resolutions to fire* — the resolution
    /// lookup silently no-ops when no registry is installed or when
    /// `module_for` returns `None`.
    pub fn with_scenario_id(mut self, id: ScenarioId) -> Self {
        self.scenario_id = Some(id);
        self
    }

    /// Build into a [`TestSession`] for driving the engine with a
    /// scripted [`ChoiceResolver`].
    ///
    /// Equivalent to `TestSession::new(self.build())`; sugar so
    /// resolver-driven tests can write
    /// `TestGame::new()...session().apply(...).resolve_choices(...).run()`.
    ///
    /// [`TestSession`]: super::resolver::TestSession
    /// [`ChoiceResolver`]: super::resolver::ChoiceResolver
    pub fn session(self) -> super::resolver::TestSession {
        super::resolver::TestSession::new(self.build())
    }

    /// Materialize the configured [`GameState`].
    pub fn build(self) -> GameState {
        GameState {
            investigators: self.investigators,
            locations: self.locations,
            enemies: self.enemies,
            chaos_bag: self.chaos_bag,
            token_modifiers: self.token_modifiers,
            phase: self.phase,
            round: self.round,
            active_investigator: self.active_investigator,
            turn_order: self.turn_order,
            rng: self.rng,
            mulligan_window: self.mulligan_window,
            next_card_instance_id: 0,
            next_enemy_id: 0,
            pending_skill_modifiers: Vec::new(),
            in_flight_skill_test: None,
            open_windows: self.open_windows,
            scenario_id: self.scenario_id,
            encounter_deck: VecDeque::new(),
            encounter_discard: Vec::new(),
        }
    }
}

impl Default for TestGame {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod with_open_window_tests {
    use super::*;
    use crate::test_support::test_investigator;

    #[test]
    fn with_open_window_pushes_onto_the_stack() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Mythos,
                    to: Phase::Investigation,
                },
                FastActorScope::Any,
            )
            .build();
        assert_eq!(state.open_windows.len(), 1);
        assert_eq!(state.open_windows[0].fast_actors, FastActorScope::Any);
        assert!(state.open_windows[0].pending_triggers.is_empty());
    }

    #[test]
    fn with_open_window_stacks_in_order() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Mythos,
                    to: Phase::Investigation,
                },
                FastActorScope::Any,
            )
            .with_open_window(
                WindowKind::BetweenPhases {
                    from: Phase::Investigation,
                    to: Phase::Enemy,
                },
                FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            )
            .build();
        assert_eq!(state.open_windows.len(), 2);
        assert!(matches!(
            state.open_windows[1].kind,
            WindowKind::BetweenPhases {
                to: Phase::Enemy,
                ..
            }
        ));
    }
}
