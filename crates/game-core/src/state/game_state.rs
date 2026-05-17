//! Top-level game state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    card::CardInstanceId,
    chaos_bag::{ChaosBag, TokenModifiers},
    enemy::{Enemy, EnemyId},
    investigator::{Investigator, InvestigatorId, SkillKind},
    location::{Location, LocationId},
    phase::Phase,
};
use crate::dsl::{SkillTestKind, Stat};
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
    /// The skill test currently paused between [`SkillTestStarted`] and
    /// [`ChaosTokenRevealed`] awaiting commit-window input from the
    /// active investigator. `Some` whenever the engine has emitted
    /// [`EngineOutcome::AwaitingInput`] for a commit window and is
    /// waiting on a
    /// [`PlayerAction::ResolveInput`](crate::action::PlayerAction::ResolveInput)
    /// with a
    /// [`CommitCards`](crate::action::InputResponse::CommitCards)
    /// response. While set, every non-`ResolveInput` player action
    /// rejects (mirrors the [`mulligan_window`](Self::mulligan_window)
    /// guard).
    ///
    /// [`SkillTestStarted`]: crate::Event::SkillTestStarted
    /// [`ChaosTokenRevealed`]: crate::Event::ChaosTokenRevealed
    /// [`EngineOutcome::AwaitingInput`]: crate::EngineOutcome::AwaitingInput
    pub in_flight_skill_test: Option<InFlightSkillTest>,
}

/// A skill test paused mid-resolution at the commit window.
///
/// Pushed by the skill-test initiator (`PerformSkillTest`, `Investigate`,
/// `Fight`, `Evade`) after [`SkillTestStarted`] fires; consumed by the
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) dispatch
/// once the active investigator submits their commit list. The follow-
/// up describes the action-specific success path: discover a clue
/// (Investigate), deal damage (Fight), disengage and exhaust (Evade),
/// or nothing (bare `PerformSkillTest`).
///
/// [`SkillTestStarted`]: crate::Event::SkillTestStarted
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InFlightSkillTest {
    /// Investigator taking the test.
    pub investigator: InvestigatorId,
    /// Skill the test is against.
    pub skill: SkillKind,
    /// Test kind (Investigate / Fight / Evade / Plain). Drives
    /// [`ModifierScope::WhileInPlayDuring`](crate::dsl::ModifierScope::WhileInPlayDuring)
    /// matching during resolution.
    pub kind: SkillTestKind,
    /// Difficulty: total to meet or exceed for success.
    pub difficulty: i8,
    /// Hand indices the active investigator has committed to the test.
    /// Populated on the [`ResolveInput`](crate::action::PlayerAction::ResolveInput)
    /// dispatch and snapshotted onto the in-flight record for replay
    /// clarity and inspection of a saved mid-test state. The icon-sum
    /// and discard paths read the indices off the local variable
    /// computed during validation, not off this field — the field is
    /// captured *afterward*, so it's not load-bearing for resolution
    /// today.
    ///
    /// Multi-investigator commits (the rule "any investigator at the
    /// same location may commit") are a separate downstream issue; for
    /// now only the active investigator's commits live here.
    pub committed_by_active: Vec<u8>,
    /// The location the test is associated with, snapshotted at
    /// skill-test start (`engine::dispatch::start_skill_test`) from
    /// the investigator's current location. Used by
    /// [`LocationTarget::TestedLocation`](crate::dsl::LocationTarget::TestedLocation)
    /// during
    /// [`Trigger::OnSkillTestResolution`](crate::dsl::Trigger::OnSkillTestResolution)
    /// firing so "at that location" resolves to the location the
    /// test was originally taken against, even if the investigator
    /// has since moved (no Phase-3 path moves mid-test, but the
    /// snapshot future-proofs against cards that will). `None` when
    /// the investigator was between locations at test start —
    /// only reachable via the bare
    /// [`PerformSkillTest`](crate::action::PlayerAction::PerformSkillTest)
    /// from outside an Investigate path.
    pub tested_location: Option<LocationId>,
    /// Action-specific resolution to apply on success.
    pub follow_up: SkillTestFollowUp,
}

/// What to do after the bracketing skill test resolves, depending on
/// which player action initiated it.
///
/// All variants are no-ops on failure (Fight / Evade / Investigate's
/// on-success effects only fire when the test succeeds; the bare
/// `PerformSkillTest` has no follow-up either way). The success-path
/// effect is what each variant captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SkillTestFollowUp {
    /// No action-specific follow-up. Used by bare
    /// [`PerformSkillTest`](crate::action::PlayerAction::PerformSkillTest).
    None,
    /// On success, discover 1 clue at the investigator's current
    /// location (via the
    /// [`DiscoverClue`](crate::dsl::Effect::DiscoverClue) evaluator
    /// path). Used by [`Investigate`](crate::action::PlayerAction::Investigate).
    Investigate,
    /// On success, deal 1 damage to the named enemy (and defeat it if
    /// damage reaches `max_health`). Used by
    /// [`Fight`](crate::action::PlayerAction::Fight).
    Fight {
        /// The enemy the Fight action targeted.
        enemy: EnemyId,
    },
    /// On success, disengage the named enemy from the investigator and
    /// exhaust it. Used by [`Evade`](crate::action::PlayerAction::Evade).
    Evade {
        /// The enemy the Evade action targeted.
        enemy: EnemyId,
    },
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
