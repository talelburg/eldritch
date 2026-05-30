//! Top-level game state.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use super::{
    card::{CardCode, CardInstanceId},
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
    /// Monotonic counter for assigning [`EnemyId`]s when enemies
    /// enter play via the encounter deck (see
    /// `crate::engine::dispatch::spawn_enemy`). Starts at 0 and
    /// increments after each assignment; guarantees uniqueness within
    /// a scenario and deterministic ids across replays.
    ///
    /// Distinct from [`next_card_instance_id`](Self::next_card_instance_id)
    /// because [`EnemyId`] and [`CardInstanceId`] are distinct types —
    /// enemies aren't tracked in the `CardInPlay` registry.
    pub next_enemy_id: u32,
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
    /// Stack of currently-open windows. The top (`last()`) is the
    /// most recently-opened; closing pops the top. Carries pending
    /// reaction triggers and the Fast-action gate for each window.
    /// Replaced the earlier single-slot `in_flight_reaction_window:
    /// Option<ReactionWindow>` shape — multi-window nesting is now
    /// structural.
    ///
    /// Window kinds open at canonical timing points:
    /// - `AfterEnemyDefeated` — queued by `damage_enemy` when an
    ///   enemy reaches 0 health.
    /// - `BetweenPhases` — opened by the phase machine at every
    ///   phase transition (Phase-4 phase-content PRs wire this).
    ///
    /// Multi-window queueing (one effect that queues two windows in
    /// the same apply) is now structural — push twice, drive resumes
    /// in reverse open order.
    pub open_windows: Vec<OpenWindow>,
    /// Identifier of the scenario this state belongs to, if any.
    ///
    /// `None` for tests and fixtures that don't care about scenario
    /// resolution; in that case the engine's post-apply resolution
    /// hook short-circuits. `Some(id)` is the normal case: the
    /// engine looks up the module via
    /// [`scenario_registry::current`](crate::scenario_registry::current)
    /// and asks it whether the new state has resolved.
    ///
    /// Serializable so action-log replay reproduces the lookup
    /// deterministically across host restarts.
    pub scenario_id: Option<crate::scenario::ScenarioId>,
    /// The investigator whose Mythos-phase encounter draw is pending,
    /// during Rules-Reference p.24 step 1.4. `Some(id)` between
    /// `mythos_phase` entry and the last drawer's completion; `None`
    /// otherwise. Advanced after each `PlayerAction::DrawEncounterCard`
    /// completes its chain (including any surge re-draws). `None`
    /// once all investigators have drawn — at which point the
    /// `MythosAfterDraws` window opens.
    pub mythos_draw_pending: Option<InvestigatorId>,
    /// The next investigator due to resolve engaged-enemy attacks
    /// during Enemy phase step 3.3. Mirror of [`mythos_draw_pending`]:
    ///
    /// - Set to the first [`Status::Active`] investigator in
    ///   [`turn_order`] when `enemy_phase` runs step 3.3's loop
    ///   kickoff.
    /// - Advanced by `run_window_continuation` after each
    ///   per-investigator attack resolution closes, to the next Active
    ///   investigator in [`turn_order`] (or `None` when the loop is
    ///   done).
    /// - Stays `None` during all phases other than Enemy.
    ///
    /// Eliminated investigators ([`Status::Killed`] / [`Status::Insane`]
    /// / [`Status::Resigned`]) are skipped during advance, mirroring
    /// the `mythos_draw_pending` semantics established in #69.
    ///
    /// [`mythos_draw_pending`]: GameState::mythos_draw_pending
    /// [`turn_order`]: GameState::turn_order
    /// [`Status::Active`]: crate::state::Status::Active
    /// [`Status::Killed`]: crate::state::Status::Killed
    /// [`Status::Insane`]: crate::state::Status::Insane
    /// [`Status::Resigned`]: crate::state::Status::Resigned
    pub enemy_attack_pending: Option<InvestigatorId>,
    /// Suspended Hunter-movement choice (#128), `Some` only while the
    /// Enemy phase is paused on a lead-investigator tie; cleared once
    /// resolved. See [`HunterChoice`].
    pub hunter_move_pending: Option<HunterChoice>,
    /// Shared encounter deck (top = front). Built at scenario setup
    /// from encounter-set codes; drawn from during Mythos. When the
    /// deck runs out, `draw_encounter_top` (in `engine::dispatch`)
    /// transparently reshuffles [`encounter_discard`](Self::encounter_discard)
    /// back in via the deterministic RNG path.
    ///
    /// Empty at the start of every scenario; populated by scenario
    /// setup (the first wiring lands in #126 alongside the synthetic
    /// fixture's encounter-set composition).
    pub encounter_deck: VecDeque<CardCode>,
    /// Encounter discard pile. Treacheries land here after Revelation
    /// resolves; defeated enemies (and other "discarded from play"
    /// encounter content) land here in later issues.
    ///
    /// Drained back into [`encounter_deck`](Self::encounter_deck) by
    /// `reshuffle_encounter_discard` (in `engine::dispatch`) when
    /// the deck runs empty.
    pub encounter_discard: Vec<CardCode>,
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
    /// Where the resolution driver should resume on the next call to
    /// `drive_skill_test`. Initialized to
    /// [`FinishContinuation::AwaitingCommit`] at
    /// `start_skill_test`; advanced in lock-step as the resolution
    /// sequence runs. Post-commit variants carry the test's outcome
    /// as a `succeeded` payload (see [`FinishContinuation`]) so the
    /// invariant "outcome is known iff the test is past the commit
    /// window" is structural.
    pub continuation: FinishContinuation,
}

/// Where the skill-test resolution driver should resume on the next
/// call to `drive_skill_test`.
///
/// The driver walks a fixed sequence of steps inside
/// `finish_skill_test`:
///
/// 1. Validate commits + draw chaos token + emit
///    [`SkillTestSucceeded`](crate::Event::SkillTestSucceeded) /
///    [`SkillTestFailed`](crate::Event::SkillTestFailed)
/// 2. Apply the action-specific
///    [`SkillTestFollowUp`] (Investigate / Fight / Evade / None) —
///    this is where `damage_enemy` may emit
///    [`EnemyDefeated`](crate::Event::EnemyDefeated) and queue an
///    [`AfterEnemyDefeated`](WindowKind::AfterEnemyDefeated) window
/// 3. Fire
///    [`OnSkillTestResolution`](crate::dsl::Trigger::OnSkillTestResolution)
///    triggers on committed cards
/// 4. Discard committed cards + emit
///    [`SkillTestEnded`](crate::Event::SkillTestEnded) + drain
///    pending modifiers
///
/// After each step that *can* queue a reaction window, the driver
/// checks `state.open_windows` via `GameState::top_reaction_window()`; if a window is
/// pending it suspends with
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput). On resume
/// (via `close_reaction_window_at`) the driver reads this field and
/// jumps to the matching step. This is the rules-correct shape per
/// the Rules Reference's "after… initiates immediately after that
/// triggering condition's impact has resolved" clause: the reaction
/// fires between steps 2 and 3, not after the entire action ends.
///
/// Variants past [`AwaitingCommit`](Self::AwaitingCommit) carry the
/// `succeeded` payload because the test's outcome is determined in
/// step 1 and read by every subsequent step
/// (`OnSkillTestResolution` gating, `#64`'s reactive after-resolution
/// window). Embedding it in the continuation makes the invariant
/// "succeeded is known iff the test is past the commit window"
/// structural.
///
/// Variants:
///
/// - [`AwaitingCommit`](Self::AwaitingCommit) — initial state at
///   skill-test start. No resume; the next dispatch step is the
///   commit-window
///   [`ResolveInput`](crate::action::PlayerAction::ResolveInput)
///   with a [`CommitCards`](crate::action::InputResponse::CommitCards)
///   response.
/// - [`PostFollowUp`](Self::PostFollowUp) — set by the commit-stage
///   entry once steps 1–2 have run. The next driver iteration runs
///   step 3.
/// - [`PostOnResolution`](Self::PostOnResolution) — set after step 3.
///   The next driver iteration runs step 4 (terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FinishContinuation {
    /// Initial state: waiting on the commit-window
    /// [`ResolveInput`](crate::action::PlayerAction::ResolveInput).
    AwaitingCommit,
    /// Steps 1–2 are complete (chaos token + action follow-up).
    /// The next driver iteration runs `OnSkillTestResolution` triggers.
    PostFollowUp {
        /// The chaos-token resolution's success determination, read by
        /// the `OnSkillTestResolution` step to gate
        /// outcome-specific triggers.
        succeeded: bool,
    },
    /// Step 3 (`OnSkillTestResolution`) is complete. The next driver
    /// iteration discards committed cards, emits
    /// [`SkillTestEnded`](crate::Event::SkillTestEnded), and clears
    /// the in-flight record.
    PostOnResolution {
        /// Carried through to the after-resolution reactive trigger
        /// window (#64), which will gate on outcome at the
        /// `SkillTestEnded` boundary.
        succeeded: bool,
    },
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

/// Which investigators may submit Fast `PlayCard` / `ActivateAbility`
/// actions while an `OpenWindow` is the top of `GameState::open_windows`.
///
/// Modeled per Rules Reference: a reaction window allows any
/// investigator to fire a triggered reaction or play a Fast card.
/// An investigator's own turn opens an `ActiveInvestigator` window
/// that still permits other investigators to play Fast cards (per the
/// "Fast may be played at any player window" rule); concrete window
/// kinds choose the right scope at the open-window site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FastActorScope {
    /// Only the named investigator may submit Fast actions during
    /// this window. Used for narrow Investigation-phase windows (the
    /// turn's owner) where Fast actions are still bounded to one
    /// actor; pair with `Any` for windows where other investigators
    /// may interject.
    ActiveInvestigator(InvestigatorId),
    /// Any investigator may submit Fast actions. Used for reaction
    /// windows and between-phase windows.
    Any,
    /// Only the named set may submit Fast actions. Reserved for
    /// scenario-specific windows that restrict actors by criterion
    /// (e.g. only investigators at a given location). No Phase-3
    /// or Phase-4 site constructs this variant yet; the variant
    /// exists so future cards can grow it without engine churn.
    Specific(std::collections::BTreeSet<InvestigatorId>),
}

impl FastActorScope {
    /// True if `investigator` is permitted to submit a Fast action
    /// during the window carrying this scope.
    #[must_use]
    pub fn permits(&self, investigator: InvestigatorId) -> bool {
        match self {
            Self::ActiveInvestigator(id) => *id == investigator,
            Self::Any => true,
            Self::Specific(set) => set.contains(&investigator),
        }
    }
}

/// A currently-open window on the action stack.
///
/// Replaces the older single-slot `in_flight_reaction_window: Option<ReactionWindow>` shape;
/// reaction-window machinery now operates on this stack via
/// `GameState::top_reaction_window()` and `top_reaction_window_mut()`.
///
/// Each window carries (a) what kind it is and which IDs the
/// triggering event/phase-transition named, (b) the queue of
/// `Trigger::OnEvent` reactions waiting to fire, and (c) which
/// investigators may submit Fast `PlayCard` / `ActivateAbility`
/// actions while this window is the top of `GameState::open_windows`.
///
/// Windows nest: a reaction firing inside another window may itself
/// trigger sub-reactions that open further windows on top of this
/// one. The dispatcher always reads / mutates the top of the stack
/// (`open_windows.last_mut()` / `open_windows.pop()`); closing a
/// window simply pops the top.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenWindow {
    /// What kind of window is open; carries the IDs the triggering
    /// event named (defeated enemy + attacker, phase transition,
    /// etc.) so pending triggers' effects can resolve against the
    /// same payload.
    pub kind: WindowKind,
    /// Triggers in resolution order. Active investigator's matching
    /// triggers come first (Arkham's "active player priority"), then
    /// other investigators' in turn order. Within a single
    /// investigator, listed in `cards_in_play` order, then by
    /// `ability_index`. Empty `pending_triggers` is permitted —
    /// windows opened for phase/timing reasons (not reaction-driven)
    /// may have no triggers but still gate Fast actions.
    pub pending_triggers: Vec<PendingTrigger>,
    /// Which investigators may submit Fast `PlayCard` /
    /// `ActivateAbility` actions while this window is the top of
    /// the stack.
    pub fast_actors: FastActorScope,
}

impl OpenWindow {
    /// Construct an empty [`OpenWindow`] (no pending triggers) for the
    /// given `kind` and `fast_actors` scope.
    ///
    /// Provided so integration tests outside the crate (where the
    /// `#[non_exhaustive]` attribute blocks struct-literal construction)
    /// can inject a window directly onto
    /// [`GameState::open_windows`] for stack-shape regression tests.
    #[must_use]
    pub fn new_empty(kind: WindowKind, fast_actors: FastActorScope) -> Self {
        Self {
            kind,
            pending_triggers: Vec::new(),
            fast_actors,
        }
    }
}

/// Discriminant of an open `OpenWindow`.
///
/// Each variant pairs with a [`Trigger::OnEvent`](crate::dsl::Trigger::OnEvent)
/// pattern: when the engine emits a matching
/// [`Event`](crate::Event), it queues a window of the corresponding
/// kind. Phase-3 starts with one variant; later cards add
/// patterns and the engine queues their matching window kind. The
/// after-skill-test reactive window
/// ([#64](https://github.com/talelburg/eldritch/issues/64)) is the
/// next planned variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowKind {
    /// Fires after an enemy was defeated. Pairs with
    /// [`EventPattern::EnemyDefeated`](crate::dsl::EventPattern::EnemyDefeated)
    /// with [`EventTiming::After`](crate::dsl::EventTiming::After).
    AfterEnemyDefeated {
        /// The defeated enemy. Carried so trigger effects keying on
        /// "the defeated enemy" can route against the right id even
        /// after `state.enemies` has dropped the entry.
        enemy: EnemyId,
        /// Who defeated it, if attributable. Mirrors the
        /// [`Event::EnemyDefeated`](crate::Event::EnemyDefeated)
        /// `by` field. `None` for non-investigator-attributed defeats.
        by: Option<InvestigatorId>,
    },
    /// A window opened between two phases. Phase-4 phase-content PRs
    /// open this at each canonical transition (e.g. before Mythos,
    /// between Investigation and Enemy) so Fast cards + cross-phase
    /// reactions fire correctly. `fast_actors` is typically `Any`.
    BetweenPhases {
        /// The phase we're leaving.
        from: Phase,
        /// The phase we're entering.
        to: Phase,
    },
    /// The player window between Rules Reference p.24 step 1.4
    /// (each investigator draws an encounter card) and step 1.5
    /// (Mythos phase ends). Carries no payload — there is no
    /// `EventPattern` today that matches against this specifically;
    /// the variant exists so the rule's printed timing point is
    /// addressable when a future card binds to it.
    MythosAfterDraws,
    /// The player window between Rules Reference p.25 step 4.1 (upkeep
    /// phase begins) and step 4.2 (reset actions). Carries no payload —
    /// no `EventPattern` matches against it specifically today; the
    /// variant exists so the rule's printed timing point is addressable
    /// when a future card binds to it. Mirror of `MythosAfterDraws`.
    UpkeepBegins,
    /// The player window opened before an investigator's engaged
    /// enemies resolve their attacks (Rules Reference p.25 step 3.3,
    /// the "previous player window" investigators "return to" between
    /// resolutions). The investigator to be attacked next is carried
    /// on [`GameState::enemy_attack_pending`], not in the variant —
    /// mirror of [`MythosAfterDraws`] + [`GameState::mythos_draw_pending`].
    ///
    /// Continuation (in `run_window_continuation`): read the cursor,
    /// resolve the pending investigator's engaged ready enemies in
    /// [`EnemyId`] order, exhaust each, advance the cursor to the next
    /// Active investigator in [`turn_order`] (or `None`), open the next
    /// window (`BeforeInvestigatorAttacked` if Some,
    /// `AfterAllInvestigatorsAttacked` if None).
    ///
    /// One window per Active investigator in `turn_order`.
    ///
    /// [`MythosAfterDraws`]: WindowKind::MythosAfterDraws
    /// [`turn_order`]: GameState::turn_order
    BeforeInvestigatorAttacked,
    /// The player window after all investigators have resolved their
    /// engaged enemies' attacks (Rules Reference p.25 step 3.3, the
    /// "next player window" entered after the final investigator).
    /// Continuation runs `enemy_phase_end` (step 3.4 + transition).
    /// Mirror of [`MythosAfterDraws`]'s end-of-step shape.
    ///
    /// [`MythosAfterDraws`]: WindowKind::MythosAfterDraws
    AfterAllInvestigatorsAttacked,
    /// The player window between Rules Reference p.24 step 2.1
    /// (Investigation phase begins) and step 2.2 (the first
    /// investigator's turn begins). Bare variant — no `EventPattern`
    /// matches it today; it exists so the printed timing point is
    /// addressable and so step 2.2's rotation runs in this window's
    /// continuation (preserving the printed 2.1 → window → 2.2 order).
    InvestigationBegins,
    /// The player window opened at the start of each investigator's
    /// turn (Rules Reference p.24 step 2.2, the "previous player window"
    /// that actions return to during step 2.2.1). Bare variant. One per
    /// investigator turn. Continuation is a no-op: the engine then waits
    /// for the active investigator's player-driven actions.
    InvestigatorTurnBegins,
}

/// A suspended Hunter-movement choice awaiting the lead investigator's
/// input during Enemy-phase step 3.2 (#128, Rules Reference p.12 / p.10 /
/// p.17).
///
/// Two shapes because the two choice points need different input:
/// movement is a `PickLocation` over a prey-filtered destination set
/// (the chosen prey doesn't persist, so picking a location is
/// outcome-equivalent to picking an investigator-then-path); engagement
/// on arrival is a `PickInvestigator` over the co-located set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HunterChoice {
    /// Lead investigator picks the hunter's destination among tied
    /// prey-legal shortest-path next steps (Rules Reference p.12).
    Move {
        /// The hunter being moved.
        enemy: EnemyId,
        /// Legal destinations to choose among (the validated option set).
        candidates: Vec<LocationId>,
    },
    /// Lead investigator picks whom the hunter engages among co-located
    /// tied prey candidates (Rules Reference p.10 / p.17).
    Engage {
        /// The hunter that arrived.
        enemy: EnemyId,
        /// Co-located investigators to choose among.
        candidates: Vec<InvestigatorId>,
    },
}

/// A single pending [`Trigger::OnEvent`](crate::dsl::Trigger::OnEvent)
/// ability waiting to fire inside an `OpenWindow`.
///
/// Resolved by [`InputResponse::PickIndex`](crate::action::InputResponse::PickIndex)
/// — the index addresses into `OpenWindow::pending_triggers`. After firing,
/// the entry is removed; the window stays open while any entries remain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PendingTrigger {
    /// The investigator whose card in play carries this trigger. The
    /// trigger's effect resolves with this investigator as the
    /// controller.
    pub controller: InvestigatorId,
    /// Which in-play instance is the source. Plumbed onto the trigger
    /// record so identical card codes across investigators (or across
    /// copies for the same investigator) resolve unambiguously.
    pub instance_id: CardInstanceId,
    /// Zero-based index into the card's
    /// [`abilities`](crate::dsl::Ability) vec. Cards may carry
    /// multiple `Trigger::OnEvent` abilities; this names which one
    /// fires.
    pub ability_index: u8,
    /// Whether the player may skip this trigger when closing the
    /// window. `false` (optional) for `[reaction]` abilities; `true`
    /// (forced) for "Forced — when …" abilities.
    ///
    /// **Phase-3 scope**: the DSL surface has no forced primitive yet
    /// (no in-scope card carries forced text), so the engine always
    /// constructs `forced: false`. The field exists so the resolution
    /// loop already understands the distinction — when the first
    /// forced card lands, only the DSL→engine translation and the
    /// scanner need to start setting this `true`.
    pub forced: bool,
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

impl GameState {
    /// The topmost open window that has unresolved reaction triggers,
    /// if any. Used by the dispatcher's "is reaction work pending?"
    /// guards. Pure Fast-gating windows (empty `pending_triggers`)
    /// are skipped — they don't block dispatch.
    #[must_use]
    pub fn top_reaction_window(&self) -> Option<&OpenWindow> {
        self.open_windows
            .iter()
            .rev()
            .find(|w| !w.pending_triggers.is_empty())
    }

    /// Mutable counterpart to `top_reaction_window`. Same skip rule
    /// applies: windows with empty `pending_triggers` are skipped —
    /// phase-gate-only windows are not exposed as reaction-work.
    pub fn top_reaction_window_mut(&mut self) -> Option<&mut OpenWindow> {
        self.open_windows
            .iter_mut()
            .rev()
            .find(|w| !w.pending_triggers.is_empty())
    }

    /// Index into [`Self::open_windows`] of the topmost window with
    /// non-empty `pending_triggers`, matching the window that
    /// [`Self::top_reaction_window`] / [`Self::top_reaction_window_mut`]
    /// resolve to.
    ///
    /// Callers driving the reaction window pass this index to
    /// `close_reaction_window_at` so the close path removes the same
    /// entry the driver was operating on, rather than blindly popping
    /// the top of the stack — a `BetweenPhases` window with empty
    /// `pending_triggers` can sit above an active reaction window,
    /// which would corrupt the stack on naive `pop()`.
    ///
    /// Note: the `Skip` path in `resolve_input` also handles **pure-Fast
    /// windows** (empty `pending_triggers`, pushed by `open_fast_window`)
    /// by closing the literal top-of-stack index directly rather than
    /// going through this helper. That path is safe because a pure-Fast
    /// window, by construction, has no forced triggers to guard against.
    #[must_use]
    pub fn top_reaction_window_index(&self) -> Option<usize> {
        self.open_windows
            .iter()
            .rposition(|w| !w.pending_triggers.is_empty())
    }
}

#[cfg(test)]
mod open_window_tests {
    use super::*;

    #[test]
    fn open_window_serde_roundtrip() {
        let window = OpenWindow {
            kind: WindowKind::AfterEnemyDefeated {
                enemy: EnemyId(7),
                by: Some(InvestigatorId(1)),
            },
            pending_triggers: Vec::new(),
            fast_actors: FastActorScope::Any,
        };
        let json = serde_json::to_string(&window).expect("serialize");
        let back: OpenWindow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, window);
    }

    #[test]
    fn between_phases_window_kind_serde_roundtrip() {
        let kind = WindowKind::BetweenPhases {
            from: Phase::Mythos,
            to: Phase::Investigation,
        };
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn mythos_after_draws_window_kind_serde_roundtrip() {
        let kind = WindowKind::MythosAfterDraws;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn upkeep_begins_window_kind_serde_roundtrip() {
        let kind = WindowKind::UpkeepBegins;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn before_investigator_attacked_window_kind_serde_roundtrip() {
        let kind = WindowKind::BeforeInvestigatorAttacked;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn after_all_investigators_attacked_window_kind_serde_roundtrip() {
        let kind = WindowKind::AfterAllInvestigatorsAttacked;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn investigation_begins_window_kind_serde_roundtrip() {
        let kind = WindowKind::InvestigationBegins;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn investigator_turn_begins_window_kind_serde_roundtrip() {
        let kind = WindowKind::InvestigatorTurnBegins;
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }
}

#[cfg(test)]
mod fast_actor_scope_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn active_investigator_permits_only_named() {
        let scope = FastActorScope::ActiveInvestigator(InvestigatorId(1));
        assert!(scope.permits(InvestigatorId(1)));
        assert!(!scope.permits(InvestigatorId(2)));
    }

    #[test]
    fn any_permits_everyone() {
        let scope = FastActorScope::Any;
        assert!(scope.permits(InvestigatorId(1)));
        assert!(scope.permits(InvestigatorId(42)));
    }

    #[test]
    fn specific_permits_only_the_named_set() {
        let mut set = BTreeSet::new();
        set.insert(InvestigatorId(1));
        set.insert(InvestigatorId(3));
        let scope = FastActorScope::Specific(set);
        assert!(scope.permits(InvestigatorId(1)));
        assert!(!scope.permits(InvestigatorId(2)));
        assert!(scope.permits(InvestigatorId(3)));
    }

    #[test]
    fn fast_actor_scope_serde_roundtrip() {
        let mut set = BTreeSet::new();
        set.insert(InvestigatorId(7));
        for scope in [
            FastActorScope::Any,
            FastActorScope::ActiveInvestigator(InvestigatorId(1)),
            FastActorScope::Specific(set),
        ] {
            let json = serde_json::to_string(&scope).expect("serialize");
            let back: FastActorScope = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, scope);
        }
    }
}

#[cfg(test)]
mod next_enemy_id_tests {
    use super::*;
    use crate::test_support::TestGame;

    #[test]
    fn game_state_has_next_enemy_id_counter_starting_at_zero() {
        let state = TestGame::new().build();
        assert_eq!(state.next_enemy_id, 0);
    }

    #[test]
    fn next_enemy_id_round_trips_through_serde() {
        let mut state = TestGame::new().build();
        state.next_enemy_id = 42;
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.next_enemy_id, 42);
    }
}

#[cfg(test)]
mod mythos_draw_pending_tests {
    use crate::test_support::TestGame;

    #[test]
    fn game_state_default_has_no_mythos_draw_pending() {
        let state = TestGame::new().build();
        assert_eq!(state.mythos_draw_pending, None);
    }
}

#[cfg(test)]
mod enemy_attack_pending_tests {
    use super::*;
    use crate::state::InvestigatorId;
    use crate::test_support::TestGame;

    #[test]
    fn game_state_default_has_no_enemy_attack_pending() {
        let state = TestGame::new().build();
        assert_eq!(state.enemy_attack_pending, None);
    }

    #[test]
    fn enemy_attack_pending_round_trips_through_serde() {
        let mut state = TestGame::new().build();
        state.enemy_attack_pending = Some(InvestigatorId(7));
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.enemy_attack_pending, Some(InvestigatorId(7)));
    }
}

#[cfg(test)]
mod encounter_deck_tests {
    use super::*;
    use crate::state::CardCode;
    use crate::test_support::TestGame;

    #[test]
    fn encounter_deck_and_discard_serde_roundtrip() {
        let mut state = TestGame::new().build();
        state.encounter_deck.push_back(CardCode("01001".into()));
        state.encounter_deck.push_back(CardCode("01002".into()));
        state.encounter_discard.push(CardCode("01099".into()));

        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.encounter_deck.len(), 2);
        assert_eq!(back.encounter_deck[0], CardCode("01001".into()));
        assert_eq!(back.encounter_deck[1], CardCode("01002".into()));
        assert_eq!(back.encounter_discard.len(), 1);
        assert_eq!(back.encounter_discard[0], CardCode("01099".into()));
    }

    #[test]
    fn fresh_state_has_empty_encounter_deck_and_discard() {
        let state = TestGame::new().build();
        assert!(state.encounter_deck.is_empty());
        assert!(state.encounter_discard.is_empty());
    }
}

#[cfg(test)]
mod hunter_pending_tests {
    use super::*;

    #[test]
    fn hunter_choice_move_serde_roundtrip() {
        let original = HunterChoice::Move {
            enemy: EnemyId(3),
            candidates: vec![LocationId(2), LocationId(3)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn hunter_choice_engage_serde_roundtrip() {
        let original = HunterChoice::Engage {
            enemy: EnemyId(5),
            candidates: vec![InvestigatorId(1), InvestigatorId(2)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: HunterChoice = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}
