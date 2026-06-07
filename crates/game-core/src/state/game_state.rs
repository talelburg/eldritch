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
/// Phase-1 minimal shape; later phases will add e.g. persistent
/// campaign-log facts and cross-scenario trauma tracking.
///
/// Investigators and locations are stored in [`BTreeMap`]s keyed by ID
/// rather than [`Vec`]s. This makes iteration order deterministic
/// (sorted by ID) regardless of insertion order — important for replay
/// equality — and gives O(log n) lookup. Turn order is tracked
/// separately in [`turn_order`](Self::turn_order); the storage map's
/// iteration order is *not* turn order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The investigator whose setup mulligan is pending, processed in
    /// player order (Rules Reference p.16 / p.27: "each player, in
    /// player order, may mulligan once"). Mirror of
    /// [`mythos_draw_pending`](Self::mythos_draw_pending):
    ///
    /// - Seeded to the first [`Status::Active`](crate::state::Status::Active)
    ///   investigator in [`turn_order`](Self::turn_order) at
    ///   [`PlayerAction::StartScenario`](crate::action::PlayerAction::StartScenario).
    /// - A [`PlayerAction::Mulligan`](crate::action::PlayerAction::Mulligan)
    ///   is valid only when `mulligan_pending == Some(that investigator)`;
    ///   on success the cursor advances to the next Active investigator
    ///   in `turn_order`.
    /// - `None` once every investigator has mulliganed — at which point
    ///   setup ends and the Investigation phase begins. While `Some`,
    ///   the engine rejects every non-Mulligan player action.
    pub mulligan_pending: Option<InvestigatorId>,
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
    /// rejects (mirrors the [`mulligan_pending`](Self::mulligan_pending)
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
    /// hook short-circuits. `Some(id)` is the normal case: on a
    /// `None`→`Some` [`resolution`](Self::resolution) latch transition the
    /// engine looks up the module via
    /// [`scenario_registry::current`](crate::scenario_registry::current)
    /// and runs its `apply_resolution`.
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
    /// Suspended engagement-on-spawn choice (#128). See [`SpawnEngagePending`].
    pub spawn_engage_pending: Option<SpawnEngagePending>,
    /// Suspended upkeep hand-size discard (#111). See [`HandSizeDiscard`].
    pub hand_size_discard_pending: Option<HandSizeDiscard>,
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
    /// The agenda deck (the doom-fueled lose track). `agenda_deck[agenda_index]`
    /// is the current agenda. Empty for tests/fixtures that don't model
    /// agendas — every agenda helper short-circuits on an empty deck.
    pub agenda_deck: Vec<Agenda>,
    /// Cursor into [`agenda_deck`](Self::agenda_deck): the current agenda.
    pub agenda_index: usize,
    /// Doom currently on the current agenda. Incremented +1 each Mythos
    /// step 1.2; reset to 0 when the agenda advances. (Doom on other
    /// cards in play is not summed yet — no corpus card carries doom.)
    pub agenda_doom: u8,
    /// The act deck (the investigator-driven win track). `act_deck[act_index]`
    /// is the current act. Empty for tests/fixtures that don't model acts.
    pub act_deck: Vec<Act>,
    /// Cursor into [`act_deck`](Self::act_deck): the current act.
    pub act_index: usize,
    /// Fire-once scenario-resolution latch. `None` until a resolution
    /// fires; set by `request_resolution` at the act/agenda resolution
    /// point or the no-remaining-players elimination step. The
    /// `apply` hook detects the `None`→`Some` transition to emit
    /// `Event::ScenarioResolved` and run `apply_resolution` exactly once
    /// (the idempotency guard formerly tracked as #131).
    pub resolution: Option<crate::scenario::Resolution>,
}

/// One agenda card's mechanically-relevant state: the doom needed to
/// advance it, and the printed `(→R#)` resolution point on its reverse
/// (if any). Card *effect* text is out of scope (per-scenario content);
/// `resolution` is the structural pointer that ends the scenario when a
/// terminal agenda advances.
///
/// Deliberately NOT `#[non_exhaustive]`: scenario setup in the
/// `scenarios` crate constructs these with struct literals, which a
/// `#[non_exhaustive]` struct forbids cross-crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agenda {
    /// Total doom in play required to advance (Rules Reference p.24
    /// step 1.3). Flat value only for now; per-investigator scaling
    /// and `Objective –` overrides are deferred until a real
    /// scenario needs them.
    pub doom_threshold: u8,
    /// The printed resolution point on this agenda's reverse. `Some` on
    /// a terminal agenda (advancing it ends the scenario); `None` on an
    /// agenda that advances to the next card.
    pub resolution: Option<crate::scenario::Resolution>,
}

/// One act card's mechanically-relevant state: the clues the group must
/// spend to advance it, and its `(→R#)` resolution point (if any). Not
/// `#[non_exhaustive]` for the same cross-crate-construction reason as
/// [`Agenda`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Act {
    /// Clues the investigators must spend to advance (Rules Reference
    /// p.3). Flat value only for now.
    pub clue_threshold: u8,
    /// The printed resolution point on this act's reverse. `Some` on a
    /// terminal act; `None` otherwise.
    pub resolution: Option<crate::scenario::Resolution>,
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
    /// A printed player window at a Rules-Reference timing step. Carries
    /// no event payload — these windows gate Fast actions (and run a
    /// per-step continuation when they close), they are not after-event
    /// reaction windows. The specific timing point is the [`PhaseStep`].
    PlayerWindow(PhaseStep),
}

/// The Rules-Reference timing step a [`WindowKind::PlayerWindow`] sits
/// at. Each step uniquely determines its phase, so the phase is not
/// carried separately (the engine reads [`GameState::phase`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PhaseStep {
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
    /// [`MythosAfterDraws`]: PhaseStep::MythosAfterDraws
    /// [`turn_order`]: GameState::turn_order
    BeforeInvestigatorAttacked,
    /// The player window after all investigators have resolved their
    /// engaged enemies' attacks (Rules Reference p.25 step 3.3, the
    /// "next player window" entered after the final investigator).
    /// Continuation runs `enemy_phase_end` (step 3.4 + transition).
    /// Mirror of [`MythosAfterDraws`]'s end-of-step shape.
    ///
    /// [`MythosAfterDraws`]: PhaseStep::MythosAfterDraws
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

/// A suspended engagement-on-spawn choice (#128, option A): a
/// multi-investigator spawn tie awaiting the lead investigator's
/// `PickInvestigator`. `investigator_to_draw` is the drawing
/// investigator whose Mythos encounter-draw chain resumes once the
/// engagement is chosen.
///
/// Distinct from [`HunterChoice`] because spawn engagement is not a
/// hunter move (it never picks a location) and its resume path
/// re-enters a different driver — the Mythos encounter-draw chain
/// rather than the Enemy-phase hunter loop. `surge` and `chain_count`
/// carry the surge-chain bookkeeping across the suspend boundary so the
/// drawing investigator's chain resumes with its cap budget intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpawnEngagePending {
    /// The spawned enemy awaiting an engagement target.
    pub enemy: EnemyId,
    /// The investigator who drew the enemy (Mythos draw resumes for them).
    pub investigator_to_draw: InvestigatorId,
    /// Co-located investigators to choose among.
    pub candidates: Vec<InvestigatorId>,
    /// Whether the spawned enemy card carries the surge keyword — i.e.
    /// whether the drawing investigator draws another encounter card
    /// once this engagement resolves.
    pub surge: bool,
    /// Surge-chain position at the point of suspension, so the resumed
    /// chain keeps counting toward `MAX_SURGE_CHAIN` rather than
    /// resetting its budget across the input round-trip.
    pub chain_count: usize,
}

/// Suspended upkeep maximum-hand-size discard (#111). `Some` only while
/// the upkeep phase is paused at step 4.5 waiting for an over-cap
/// investigator to choose discards; cleared once the queue drains.
///
/// `remaining[0]` is the investigator currently prompted. The queue is
/// the player-order list of over-cap investigators, precomputed once
/// when step 4.5 fires — discarding only ever shrinks the discarding
/// investigator's own hand, so no other investigator's over-cap status
/// can change mid-resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HandSizeDiscard {
    /// Over-cap investigators in player order; front = currently prompted.
    pub remaining: Vec<InvestigatorId>,
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
/// Pushed by `apply_effect` when an
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
    fn player_window_kind_serde_roundtrip() {
        let kind = WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws);
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

    #[test]
    fn spawn_engage_pending_serde_roundtrip() {
        let original = SpawnEngagePending {
            enemy: EnemyId(2),
            investigator_to_draw: InvestigatorId(1),
            candidates: vec![InvestigatorId(1), InvestigatorId(2)],
            surge: false,
            chain_count: 1,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: SpawnEngagePending = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}

#[cfg(test)]
mod hand_size_discard_tests {
    use super::*;

    #[test]
    fn hand_size_discard_serde_roundtrip() {
        let original = HandSizeDiscard {
            remaining: vec![InvestigatorId(1), InvestigatorId(2)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: HandSizeDiscard = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}

#[cfg(test)]
mod partial_eq_tests {
    use super::*;

    #[test]
    fn game_state_is_partial_eq() {
        fn assert_partial_eq<T: PartialEq>() {}
        assert_partial_eq::<GameState>();
    }
}
