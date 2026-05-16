//! Top-level game state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    card::CardInstanceId,
    chaos_bag::{ChaosBag, TokenModifiers},
    enemy::{Enemy, EnemyId},
    investigator::{Investigator, InvestigatorId},
    location::{Location, LocationId},
    phase::Phase,
};
use crate::dsl::Stat;
use crate::rng::RngState;

/// The full state of a scenario at a single point in time.
///
/// `GameState` is the world the engine mutates by applying actions.
/// In the event-sourced model, the canonical state is *derived* by
/// replaying the action log; `GameState` is the materialized cache.
///
/// Phase-1 minimal shape; later phases will add e.g. encounter deck,
/// act/agenda decks, doom track, persistent campaign-log facts, etc.
///
/// Investigators and locations are stored in [`BTreeMap`]s keyed by ID
/// rather than [`Vec`]s. This makes iteration order deterministic
/// (sorted by ID) regardless of insertion order — important for replay
/// equality — and gives O(log n) lookup. Turn order is tracked
/// separately in [`turn_order`](Self::turn_order); the storage map's
/// iteration order is *not* turn order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GameState {
    /// All investigators currently in the scenario, keyed by ID.
    pub investigators: BTreeMap<InvestigatorId, Investigator>,
    /// All locations laid out (revealed and unrevealed alike), keyed by ID.
    pub locations: BTreeMap<LocationId, Location>,
    /// All enemies currently in play, keyed by ID. Defeated enemies are
    /// removed; the map is the source of truth for "this enemy exists."
    pub enemies: BTreeMap<EnemyId, Enemy>,
    /// The chaos bag at this scenario's difficulty.
    pub chaos_bag: ChaosBag,
    /// Per-scenario numeric values for the four symbol tokens
    /// (Skull/Cultist/Tablet/ElderThing). Set at scenario setup,
    /// immutable for the scenario.
    pub token_modifiers: TokenModifiers,
    /// Current round phase.
    pub phase: Phase,
    /// 1-based round counter, incremented on each Mythos phase entry.
    pub round: u32,
    /// Whose turn it is during the [`Investigation`] phase, if any.
    /// `None` outside of Investigation.
    ///
    /// [`Investigation`]: Phase::Investigation
    pub active_investigator: Option<InvestigatorId>,
    /// Order in which investigators take their turns during the
    /// Investigation phase, as decided by the lead investigator each
    /// round. The first entry is the first to act.
    pub turn_order: Vec<InvestigatorId>,
    /// Deterministic RNG state. Carries `(seed, draws)` only; the
    /// underlying [`rand_chacha::ChaCha8Rng`] is reconstructed on
    /// demand by [`RngState`] methods.
    pub rng: RngState,
    /// Whether the mulligan setup window is open. Set true at the end
    /// of [`PlayerAction::StartScenario`](crate::action::PlayerAction::StartScenario)
    /// processing; cleared once every investigator has
    /// `mulligan_used == true`. While open, investigators may submit
    /// [`PlayerAction::Mulligan`](crate::action::PlayerAction::Mulligan)
    /// to redraw a subset of their starting hand; the engine rejects
    /// every non-Mulligan player action until the window closes.
    pub mulligan_window: bool,
    /// Monotonic counter for assigning [`CardInstanceId`]s when cards
    /// enter play. Starts at 0 and increments after each assignment;
    /// guarantees uniqueness within a scenario and deterministic ids
    /// across replays.
    pub next_card_instance_id: u32,
    /// In-flight skill modifiers contributed by activated / triggered
    /// abilities with [`ModifierScope::ThisSkillTest`] scope.
    /// Accumulates between activation and skill-test resolution; the
    /// skill-test handler drains the entries for the resolving
    /// investigator after [`Event::SkillTestEnded`] fires.
    ///
    /// [`ModifierScope::ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
    /// [`Event::SkillTestEnded`]: crate::Event::SkillTestEnded
    pub pending_skill_modifiers: Vec<PendingSkillModifier>,
}

/// A queued [`ModifierScope::ThisSkillTest`] contribution waiting to
/// apply to a skill test.
///
/// Pushed by [`apply_effect`](crate::engine::apply_effect) when an
/// activated or triggered ability resolves a `Modify { scope:
/// ThisSkillTest, … }` effect. Consumed (and cleared) by the next
/// skill-test resolution for the same investigator.
///
/// [`ModifierScope::ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PendingSkillModifier {
    /// The investigator whose skill test this contributes to.
    pub investigator: InvestigatorId,
    /// Which stat the modifier targets (the skill-test handler
    /// maps `SkillKind` → `Stat` for matching).
    pub stat: Stat,
    /// Signed magnitude.
    pub delta: i8,
    /// The in-play instance that produced the modifier, if any.
    /// `None` for modifiers from non-activated paths (e.g. an
    /// `OnPlay` ability that pushes a per-test buff). Limit-once-
    /// per-test logic in later cycles (Roland Banks, Hard Knocks
    /// upgrades) will key off this.
    pub source: Option<CardInstanceId>,
}
