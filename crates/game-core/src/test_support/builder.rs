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

use std::collections::BTreeMap;

use crate::rng::RngState;
use crate::state::{
    ChaosBag, GameState, Investigator, InvestigatorId, Location, Phase, TokenModifiers,
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
    chaos_bag: ChaosBag,
    token_modifiers: TokenModifiers,
    phase: Phase,
    round: u32,
    active_investigator: Option<InvestigatorId>,
    turn_order: Vec<InvestigatorId>,
    rng: RngState,
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
            chaos_bag: ChaosBag::new([]),
            token_modifiers: TokenModifiers::default(),
            phase: Phase::Mythos,
            round: 0,
            active_investigator: None,
            turn_order: Vec::new(),
            rng: RngState::new(0),
        }
    }

    /// Add an investigator to the state. Replaces any existing entry
    /// with the same id.
    pub fn with_investigator(mut self, investigator: Investigator) -> Self {
        self.investigators.insert(investigator.id, investigator);
        self
    }

    /// Add a location to the state. Replaces any existing entry with
    /// the same id.
    pub fn with_location(mut self, location: Location) -> Self {
        self.locations.insert(location.id, location);
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

    /// Materialize the configured [`GameState`].
    pub fn build(self) -> GameState {
        GameState {
            investigators: self.investigators,
            locations: self.locations,
            chaos_bag: self.chaos_bag,
            token_modifiers: self.token_modifiers,
            phase: self.phase,
            round: self.round,
            active_investigator: self.active_investigator,
            turn_order: self.turn_order,
            rng: self.rng,
        }
    }
}

impl Default for TestGame {
    fn default() -> Self {
        Self::new()
    }
}
