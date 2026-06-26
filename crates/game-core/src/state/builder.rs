//! Fluent [`GameStateBuilder`].
//!
//! The cross-crate constructor for [`GameState`] (which is
//! `#[non_exhaustive]`, so it can't be struct-literalled outside this
//! crate). Scenario `setup()` functions use it to build the initial
//! board, and it is also the single most-used piece of engine test
//! infrastructure — every card, scenario, and engine test depends on it,
//! so if the API is awkward, every caller pays. Keep it ergonomic.
//!
//! # Example
//!
//! ```
//! use game_core::{
//!     TurnAction, InvestigatorId, Phase,
//!     state::{GameStateBuilder, Continuation, InvestigationResume},
//!     test_support::{take_turn_action, test_investigator, test_location},
//! };
//!
//! let state = GameStateBuilder::new()
//!     .with_phase(Phase::Investigation)
//!     .with_investigator(test_investigator(1))
//!     .with_location(test_location(10, "Study"))
//!     .with_active_investigator(InvestigatorId(1))
//!     // A state constructed mid-phase needs its phase anchor (slice 1a).
//!     .with_phase_anchor(Continuation::InvestigationPhase {
//!         resume: InvestigationResume::TurnBegins,
//!     })
//!     // ...and the open-turn frame above it (slice 2a-i), popped by EndTurn.
//!     .with_investigator_turn(InvestigatorId(1))
//!     .build();
//!
//! let result = take_turn_action(state, &TurnAction::EndTurn);
//! assert!(!matches!(result.outcome, game_core::EngineOutcome::Rejected { .. }));
//! ```

use std::collections::{BTreeMap, VecDeque};

use crate::rng::RngState;
use crate::scenario::ScenarioId;
use crate::state::{
    ChaosBag, Continuation, Counter, Enemy, EnemyId, FastActorScope, FastWindowKind, GameState,
    HandSizeDiscard, Investigator, InvestigatorId, Location, LocationId, Phase, TokenModifiers,
};

/// Fluent builder for a [`GameState`].
///
/// Construct with [`GameStateBuilder::new`], chain `.with_*` setters and
/// adders, then call [`build`](GameStateBuilder::build) to get a `GameState`
/// ready for [`apply`](crate::apply).
#[derive(Debug, Clone)]
#[must_use = "GameStateBuilder is a builder; call .build() to produce a GameState"]
pub struct GameStateBuilder {
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
    mulligan_remaining: Option<Vec<InvestigatorId>>,
    mythos_draw_remaining: Option<Vec<InvestigatorId>>,
    hand_size_discard_pending: Option<HandSizeDiscard>,
    open_windows: Vec<Continuation>,
    phase_anchor: Option<Continuation>,
    investigator_turn: Option<InvestigatorId>,
    scenario_id: Option<ScenarioId>,
}

impl GameStateBuilder {
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
            mulligan_remaining: None,
            mythos_draw_remaining: None,
            hand_size_discard_pending: None,
            open_windows: Vec::new(),
            phase_anchor: None,
            investigator_turn: None,
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
    ///     test_support::{test_investigator, test_location, GameStateBuilder},
    /// };
    ///
    /// let state = GameStateBuilder::new()
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

    /// Stage a pending setup mulligan over the given player-order queue
    /// (front = currently prompted). By default no mulligan is staged so
    /// tests don't accidentally exercise Mulligan paths; opt in when a test
    /// wants to resume the mulligan loop directly via `ResolveInput` without
    /// going through scenario setup (via `seat_and_open`). The queue must list
    /// the investigators in `turn_order` (set via [`with_turn_order`](Self::with_turn_order))
    /// so the loop advances correctly. Stages a [`Continuation::Mulligan`] frame at
    /// [`build`](Self::build).
    pub fn with_mulligan_remaining(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.mulligan_remaining = Some(remaining.into_iter().collect());
        self
    }

    /// Stage a pending Mythos step-1.4 encounter draw over the given
    /// player-order queue (front = currently prompted). By default none is
    /// staged; opt in when a test wants to resume the encounter-draw loop
    /// directly via `ResolveInput(Confirm)` without driving the full Mythos
    /// cascade. The queue must list the investigators in `turn_order` (set via
    /// [`with_turn_order`](Self::with_turn_order)) so the loop advances
    /// correctly. Stages a [`Continuation::EncounterDraw`] frame at
    /// [`build`](Self::build).
    pub fn with_mythos_draw_remaining(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.mythos_draw_remaining = Some(remaining.into_iter().collect());
        self
    }

    /// Seed a pending upkeep hand-size discard for the given player-order
    /// queue (front = currently prompted).
    pub fn with_hand_size_discard_pending(
        mut self,
        remaining: impl IntoIterator<Item = InvestigatorId>,
    ) -> Self {
        self.hand_size_discard_pending = Some(HandSizeDiscard {
            remaining: remaining.into_iter().collect(),
        });
        self
    }

    /// Push a framework [`FastWindow`](Continuation::FastWindow) onto the
    /// build's window stack for tests that need a specific window-state shape.
    ///
    /// The pushed window has no pending candidates (test paths that
    /// also need a reaction queue should manipulate `state` after
    /// `build()` rather than complicate this builder).
    pub fn with_open_window(mut self, kind: FastWindowKind, fast_actors: FastActorScope) -> Self {
        // Framework player windows are `FastWindow` (#433 A-ii). The builder
        // only constructs framework windows; event windows / the forced run
        // (`TimingPointWindow`) are produced by the engine, not seeded here.
        self.open_windows.push(Continuation::FastWindow {
            candidates: Vec::new(),
            fast_actors,
            kind,
        });
        self
    }

    /// Stage a `*Phase` anchor frame (slice 1a, #393) at the **bottom** of the
    /// continuation stack — the realistic invariant for a state constructed
    /// mid-phase (the real driver pushes the anchor at phase entry, beneath any
    /// framework windows). Tests that drive `end_turn` / open phase windows
    /// without going through the phase driver use this to satisfy the anchor
    /// invariant. Panics if `c` is not a `*Phase` anchor variant.
    pub fn with_phase_anchor(mut self, c: Continuation) -> Self {
        assert!(
            matches!(
                c,
                Continuation::MythosPhase { .. }
                    | Continuation::InvestigationPhase { .. }
                    | Continuation::EnemyPhase { .. }
                    | Continuation::UpkeepPhase { .. }
            ),
            "with_phase_anchor expects a *Phase anchor variant, got {c:?}",
        );
        self.phase_anchor = Some(c);
        self
    }

    /// Stage an [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame
    /// (slice 2a-i, #393) on top of the staged `*Phase` anchor — the realistic
    /// invariant for a state constructed mid-turn (the real driver pushes it once
    /// the `InvestigatorTurnBegins` window closes). Pair with
    /// `with_phase_anchor(InvestigationPhase { resume: TurnBegins })`.
    pub fn with_investigator_turn(mut self, investigator: InvestigatorId) -> Self {
        self.investigator_turn = Some(investigator);
        self
    }

    /// Set the scenario id this state belongs to. `None` (the
    /// default from [`GameStateBuilder::new`]) means the engine's post-apply
    /// resolution hook will short-circuit. Passing a `ScenarioId`
    /// means a `ScenarioRegistry` capable of resolving it must be
    /// installed *if you want resolutions to fire* — the resolution
    /// lookup silently no-ops when no registry is installed or when
    /// `module_for` returns `None`.
    pub fn with_scenario_id(mut self, id: ScenarioId) -> Self {
        self.scenario_id = Some(id);
        self
    }

    /// Materialize the configured [`GameState`].
    pub fn build(self) -> GameState {
        // Builder-staged windows become `Resolution` frames on the one
        // continuation stack (Axis-B T3); a staged hand-size discard becomes a
        // `HandSizeDiscard` frame on top of them (#348).
        // A staged `*Phase` anchor (slice 1a, #393) sits at the bottom of the
        // stack — beneath any windows, which open *above* it during the phase.
        let mut continuations: Vec<Continuation> = self.phase_anchor.into_iter().collect();
        // A staged open turn (slice 2a-i, #393) sits directly above the anchor;
        // any window opened during the turn is a sub-resolution above it.
        if let Some(investigator) = self.investigator_turn {
            continuations.push(Continuation::InvestigatorTurn {
                investigator,
                ending: false,
            });
        }
        continuations.extend(self.open_windows);
        if let Some(hsd) = self.hand_size_discard_pending {
            continuations.push(Continuation::HandSizeDiscard(hsd));
        }
        // A staged setup mulligan becomes a `Mulligan` frame (#348). Setup and
        // upkeep are disjoint phases, so this never coexists with a staged
        // hand-size discard; push order is immaterial.
        if let Some(remaining) = self.mulligan_remaining {
            continuations.push(Continuation::Mulligan { remaining });
        }
        // A staged Mythos encounter draw becomes an `EncounterDraw` frame
        // (#348). Disjoint from setup/upkeep, so push order is immaterial.
        if let Some(remaining) = self.mythos_draw_remaining {
            continuations.push(Continuation::EncounterDraw { remaining });
        }
        GameState {
            investigators: self.investigators,
            locations: self.locations,
            set_aside_locations: Vec::new(),
            set_aside_enemies: Vec::new(),
            starting_location: None,
            enemies: self.enemies,
            chaos_bag: self.chaos_bag,
            token_modifiers: self.token_modifiers,
            phase: self.phase,
            round: self.round,
            active_investigator: self.active_investigator,
            turn_order: self.turn_order,
            rng: self.rng,
            card_instance_ids: Counter::new(),
            enemy_ids: Counter::new(),
            location_ids: Counter::new(),
            pending_skill_modifiers: Vec::new(),
            continuations,
            scenario_id: self.scenario_id,
            pending_cancellation: false,
            pending_played_event: None,
            skill_substitutions: Vec::new(),
            encounter_deck: VecDeque::new(),
            encounter_discard: Vec::new(),
            agenda_deck: Vec::new(),
            agenda_index: 0,
            agenda_doom: 0,
            act_deck: Vec::new(),
            act_index: 0,
            resolution: None,
            victory_display: Vec::new(),
        }
    }
}

impl Default for GameStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod set_aside_locations_tests {
    use super::*;

    #[test]
    fn build_starts_with_empty_set_aside_locations() {
        let state = GameStateBuilder::new().build();
        assert!(state.set_aside_locations.is_empty());
    }
}

#[cfg(test)]
mod with_open_window_tests {
    use super::*;
    use crate::state::PhaseStep;
    use crate::test_support::test_investigator;

    #[test]
    fn with_open_window_pushes_onto_the_stack() {
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
                FastActorScope::Any,
            )
            .build();
        assert_eq!(state.open_windows().len(), 1);
        assert!(matches!(
            state.open_windows()[0],
            Continuation::FastWindow {
                fast_actors: FastActorScope::Any,
                ..
            }
        ));
        assert!(state.open_windows()[0]
            .pending_candidates()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn with_open_window_stacks_in_order() {
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                FastActorScope::Any,
            )
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
                FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            )
            .build();
        assert_eq!(state.open_windows().len(), 2);
        assert!(matches!(
            state.open_windows()[1],
            Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
                ..
            }
        ));
    }
}
