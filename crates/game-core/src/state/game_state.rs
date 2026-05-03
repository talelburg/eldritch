//! Top-level game state.

use serde::{Deserialize, Serialize};

use super::{
    chaos_bag::ChaosBag,
    investigator::{Investigator, InvestigatorId},
    location::Location,
    phase::Phase,
};

/// The full state of a scenario at a single point in time.
///
/// `GameState` is the world the engine mutates by applying actions.
/// In the event-sourced model, the canonical state is *derived* by
/// replaying the action log; `GameState` is the materialized cache.
///
/// Phase-1 minimal shape; later phases will add e.g. encounter deck,
/// act/agenda decks, doom track, persistent campaign-log facts, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    /// All investigators currently in the scenario.
    pub investigators: Vec<Investigator>,
    /// All locations laid out (revealed and unrevealed alike).
    pub locations: Vec<Location>,
    /// The chaos bag at this scenario's difficulty.
    pub chaos_bag: ChaosBag,
    /// Current round phase.
    pub phase: Phase,
    /// 1-based round counter, incremented on each Mythos phase entry.
    pub round: u32,
    /// Whose turn it is during the [`Investigation`] phase, if any.
    /// `None` outside of Investigation.
    ///
    /// [`Investigation`]: Phase::Investigation
    pub active_investigator: Option<InvestigatorId>,
}
