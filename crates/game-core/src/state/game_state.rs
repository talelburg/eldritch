//! Top-level game state.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use super::{
    card::{CardCode, CardInstanceId},
    chaos_bag::{ChaosBag, TokenModifiers},
    counter::Counter,
    enemy::{Enemy, EnemyId},
    investigator::{Investigator, InvestigatorId},
    location::{Location, LocationId},
    phase::Phase,
};
use crate::card_data::{CardKind, CardMetadata};
use crate::dsl::{SkillTestKind, Stat};
use crate::rng::RngState;
use card_dsl::card_data::SkillKind;

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
    /// Locations set aside, out of play (Rules Reference p.3, "set
    /// aside"). Brought into play by card effects — The Gathering's
    /// Act-1 reverse drains these into play (the `01108:board-build`
    /// native effect).
    pub set_aside_locations: Vec<Location>,
    /// Enemies set aside, out of play (Rules Reference p.3, "set aside"),
    /// recorded by printed code only — their stats (per-investigator
    /// health, combat) are minted from the corpus at spawn time, when the
    /// investigator count is known. Brought into play by card effects —
    /// The Gathering's Act-2 reverse spawns the Ghoul Priest (01116) from
    /// here (the `01109:reverse` native effect, via [`spawn_set_aside_enemy`]).
    ///
    /// [`spawn_set_aside_enemy`]: crate::engine::spawn_set_aside_enemy
    pub set_aside_enemies: Vec<CardCode>,
    /// Where roster-seated investigators are placed at scenario start.
    /// `setup()` sets it (e.g. The Gathering -> the Study); the
    /// `StartScenario` seating step reads it. `None` leaves seated
    /// investigators unplaced (`current_location: None`) — the legacy
    /// pre-seated test path, where `setup()` already placed them.
    pub starting_location: Option<LocationId>,
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
    /// Allocator for [`CardInstanceId`]s, minted when cards enter play.
    /// Deterministic across replays; serializes as a bare `u32`.
    pub card_instance_ids: Counter<CardInstanceId>,
    /// Allocator for [`EnemyId`]s, minted when enemies enter play via the
    /// encounter deck (see `crate::engine::dispatch::spawn_enemy`).
    /// Independent of [`card_instance_ids`](Self::card_instance_ids) — the
    /// phantom-typed [`Counter`] mints only `EnemyId`s.
    pub enemy_ids: Counter<EnemyId>,
    /// Allocator for [`LocationId`]s, minted as scenarios build their board.
    pub location_ids: Counter<LocationId>,
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
    /// - `PlayerWindow` — a printed player window at a Rules-Reference
    ///   timing step (e.g. `MythosAfterDraws`), opened by the phase
    ///   machine; gates Fast actions and runs a per-step continuation.
    ///
    /// Multi-window queueing (one effect that queues two windows in
    /// the same apply) is now structural — push twice, drive resumes
    /// in reverse open order.
    /// The single suspend/resume stack (umbrella §1 / Axis-B): the top
    /// frame is resumed by `resolve_input`, taking priority over the
    /// legacy `pending_*` modes. Open reaction/fast windows live here as
    /// [`Continuation::Resolution`] frames (the former `open_windows` Vec,
    /// absorbed into the one stack). `#[serde(default)]` so pre-field
    /// states still load. Inspect windows via [`Self::open_windows`] /
    /// [`Self::top_reaction_window`] / [`Self::top_window`].
    #[serde(default)]
    pub continuations: Vec<Continuation>,
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
    /// Suspended end-of-turn continuation (C4c, #235): `Some(active)` while
    /// `end_turn` is paused on a suspending `EndOfTurn` forced effect
    /// (Frozen in Fear 01164's willpower test). The skill-test commit-resume
    /// path re-enters `resume_end_turn` to run the stranded rotation /
    /// phase-end once the test resolves. Defaults to `None`.
    #[serde(default)]
    pub pending_end_turn: Option<InvestigatorId>,
    /// Suspended upkeep hand-size discard (#111). See [`HandSizeDiscard`].
    pub hand_size_discard_pending: Option<HandSizeDiscard>,
    /// Suspended act round-end clue-spend window (#275). `Some` only while
    /// awaiting the group's Confirm/Skip at the end of the round. See
    /// [`ActRoundEndPending`].
    pub act_round_end_pending: Option<ActRoundEndPending>,
    /// `Some` while an enemy-attack loop is suspended on a soak reaction
    /// window (C5b #237). Mirror of [`pending_end_turn`](Self::pending_end_turn).
    #[serde(default)]
    pub pending_enemy_attack: Option<PendingEnemyAttack>,
    /// Set by [`Effect::Cancel`](crate::dsl::Effect::Cancel) while a
    /// Before-timing reaction window resolves; read-and-cleared by the emit
    /// site (the enemy-attack loop, `discover_clue`) after the window closes,
    /// to skip the prevented impact (Axis D #336). A bool suffices because
    /// Before-windows do not nest in scope — exactly one cancellable impact is
    /// ever in flight. TODO(#367): typed marker once Before-windows can nest.
    #[serde(default)]
    pub pending_cancellation: bool,
    /// A treachery whose Revelation suspended (e.g. initiated a skill
    /// test) and must be pushed to [`encounter_discard`](Self::encounter_discard)
    /// once the suspending sub-resolution completes. Set by
    /// `resolve_encounter_card` (in `engine::dispatch`) when its
    /// Revelation loop yields `AwaitingInput`; flushed by the skill-test
    /// driver's terminal teardown step. `None` for the common
    /// Investigate/Fight/Evade test (no pending revelation).
    /// TODO(#380): generalize beyond skill-test-suspended revelations —
    /// `ChooseOne` can now suspend mid-resolution (#350), so this side
    /// channel can fold onto the continuation stack (coordinates with #348).
    pub pending_revelation_discard: Option<CardCode>,
    /// An event card mid-play: it has left hand ("commences being played",
    /// RR Appendix I step 3) but is not yet in discard. The apply loop
    /// flushes it to the owner's discard pile on `Done` (step 4: the event is
    /// placed in discard "simultaneously with the completion" of its effect),
    /// so an `OnPlay` effect that suspends — Dynamite Blast 01024's location
    /// choice — discards the event when it resumes rather than stranding it in
    /// hand. The player-event analogue of
    /// [`pending_revelation_discard`](Self::pending_revelation_discard). `None`
    /// outside an in-flight event play.
    #[serde(default)]
    pub pending_played_event: Option<(InvestigatorId, CardCode)>,
    /// Active round-scoped skill substitutions (Mind over Matter 01036).
    /// While present, the owning investigator may make a `for_skills` test as
    /// a `use_skill` test instead (offered at test initiation). Cleared at the
    /// round boundary ("until the end of the round").
    #[serde(default)]
    pub skill_substitutions: Vec<SkillSubstitution>,
    /// Set while a skill test is paused on its "use X in place of Y?" prompt at
    /// initiation (Mind over Matter 01036). Routes the next `ResolveInput` to
    /// `resume_substitution_choice`; holds the test's investigator. `None`
    /// otherwise.
    #[serde(default)]
    pub pending_substitution_prompt: Option<InvestigatorId>,
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
    /// The victory display (Rules Reference p.21): an out-of-play zone of
    /// cards worth experience, scored at scenario end. Victory-point
    /// locations are placed here when the scenario resolves (in play +
    /// revealed + no clues); victory-point enemies enter as defeated
    /// (C3). Phase 9 sums these cards' corpus victory values for XP.
    pub victory_display: Vec<CardCode>,
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
    /// The encounter-card code this agenda is printed on (e.g.
    /// `01105`). Lets the trigger dispatcher resolve the agenda's
    /// `Trigger::OnEvent` abilities through the card registry — the
    /// agenda owns its Forced effects like any other card.
    pub code: CardCode,
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
    /// The encounter-card code this act is printed on (e.g. `01108`).
    /// Lets the trigger dispatcher resolve the act's `Trigger::OnEvent`
    /// abilities through the card registry.
    pub code: CardCode,
    /// Clues the investigators must spend to advance (Rules Reference
    /// p.3). Flat value only for now.
    pub clue_threshold: u8,
    /// The printed resolution point on this act's reverse. `Some` on a
    /// terminal act; `None` otherwise.
    pub resolution: Option<crate::scenario::Resolution>,
    /// When `Some`, this act offers a round-end clue-spend objective
    /// instead of an Investigation-phase `AdvanceAct` (see [`RoundEndAdvance`]).
    /// `None` for acts that advance by the normal action or a forced trigger.
    pub round_end_advance: Option<RoundEndAdvance>,
}

/// A round-end "may spend clues to advance" objective (Rules Reference:
/// act objectives). 01109 "The Barrier": investigators in the Hallway may,
/// as a group, spend the act's `clue_threshold` clues to advance when the
/// round ends. Generic mechanics — only the contributor location is
/// card-specific, so it is set by content (`the_gathering.rs`), not parsed
/// from the corpus (no structured `ArkhamDB` field exists for it; single
/// consumer). See issue #275.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundEndAdvance {
    /// Only investigators at this in-play location (by printed code) may
    /// contribute clues — 01109: the Hallway `01112`.
    pub contributor_location: CardCode,
}

/// A parked act round-end clue-spend window (see [`RoundEndAdvance`]). The
/// decision context is snapshotted at park time; resolved via
/// `resume_act_round_end_advance`. `Some` on [`GameState`] only while
/// awaiting the group's Confirm/Skip at the end of the round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActRoundEndPending {
    /// In-play location whose investigators may contribute clues.
    pub contributor_location: LocationId,
    /// Clues to spend to advance (the act's `clue_threshold`).
    pub threshold: u8,
}

/// Which driver to resume after a mid-attack reaction window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyAttackSource {
    /// Enemy-phase step 3.3 (`resolve_attacks_for_investigator`).
    EnemyPhase,
    /// Attack of opportunity (`fire_attacks_of_opportunity`).
    AttackOfOpportunity,
}

/// Which point in the per-attacker sequence a parked enemy-attack loop
/// suspended at (Axis D #336).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackLoopPhase {
    /// Suspended on the `BeforeEnemyAttack` cancel window, *before* the head
    /// attacker dealt damage. Resume reads `pending_cancellation`, then deals
    /// (or skips) and exhausts the head attacker.
    BeforeAttack,
    /// Suspended on the `AfterEnemyAttackDamagedAsset` soak window, *after*
    /// the head attacker dealt + exhausted. Resume drains the rest (the
    /// pre-Axis-D behavior).
    AfterSoak,
}

/// An active "use X in place of Y" skill substitution (Mind over Matter
/// 01036). Round-scoped: cleared at the round boundary. While present, the
/// owning `investigator` may make a `for_skills` test as a `use_skill` test
/// instead — the choice is offered at test initiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSubstitution {
    /// Whose tests this substitution applies to.
    pub investigator: InvestigatorId,
    /// The skill used in place of `for_skills` (Mind over Matter: Intellect).
    pub use_skill: SkillKind,
    /// The skills that may be replaced (Mind over Matter: Combat, Agility).
    pub for_skills: Vec<SkillKind>,
}

/// A parked enemy-attack loop, suspended because an attack opened a reaction
/// window — either the soak window (`AfterEnemyAttackDamagedAsset`, after
/// damage; C5b #237) or the before-attack cancel window (`BeforeEnemyAttack`,
/// before damage; Axis D #336), distinguished by [`Self::phase`]. Resumed by
/// `resume_enemy_attack` once the window closes — the same suspend/resume
/// shape as [`GameState::pending_end_turn`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEnemyAttack {
    /// The investigator whose engaged enemies are attacking.
    pub investigator: InvestigatorId,
    /// Attackers not yet resolved, in resolution order. The current attacker
    /// is still at the head for [`AttackLoopPhase::BeforeAttack`] (it has not
    /// dealt yet); already removed for [`AttackLoopPhase::AfterSoak`].
    pub remaining_attackers: Vec<EnemyId>,
    /// Which loop to re-enter.
    pub source: EnemyAttackSource,
    /// Where in the per-attacker sequence the loop suspended (Axis D #336).
    pub phase: AttackLoopPhase,
}

/// A frame on the [`GameState::continuations`] suspend/resume stack
/// (umbrella §1 / Axis-B): a typed resume point, not a closure, so it
/// serializes for replay/persistence like every other state field.
///
/// Task 3 adds the first variant (`Resolution`, an open reaction/fast
/// window — see [`ResolutionFrame`]); Task 4 adds `SkillTest`, and Axis A adds
/// `Choice`. The reaction window is just "paused, the player may act here,
/// resume on act/pass," so it is a continuation frame: this absorbs the
/// former `open_windows` Vec into the one stack (umbrella §1 — no separate
/// window structure to keep in sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Continuation {
    /// One iterative resolution run on the stack: a reaction / fast /
    /// framework window, **or** the forced run (`window: None`). The lead/
    /// player resolves its `pending_triggers` one at a time; see
    /// [`ResolutionFrame`].
    Resolution(ResolutionFrame),
    /// A skill test is mid-resolution. A resume-handle only — the test's
    /// data lives in the singleton [`GameState::in_flight_skill_test`]
    /// field (read by many call sites; no nesting today), so this frame
    /// carries no payload. Pushed when the test starts (parking at its
    /// commit window) and popped when it fully resolves (Axis-B T4).
    SkillTest,
    /// A controller choice is mid-resolution (Axis A): the effect tree is
    /// re-run from the top on each resume, replaying `decisions` to reach
    /// the next un-ground choice. See [`ChoiceFrame`].
    Choice(ChoiceFrame),
}

/// A controller choice paused mid-resolution (umbrella §3, Axis A).
///
/// The frame stores the picks made so far (`decisions`), the option ids
/// offered at the *current* suspend (`offered`, so resume validates
/// membership), the root [`Effect`](card_dsl::dsl::Effect) being resolved,
/// and the [`EvalContext`](crate::engine::EvalContext) ingredients to rebuild
/// on resume (`controller` + `source`) — mirroring how [`InFlightSkillTest`]
/// stores `investigator` + `source` rather than a non-serializable
/// `EvalContext` (see the Axis-A spec §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChoiceFrame {
    /// Picks recorded so far, in choice-encounter (pre-order) order.
    pub decisions: Vec<crate::engine::OptionId>,
    /// Option ids offered at the current suspend; resume rejects an id not
    /// in this set.
    pub offered: Vec<crate::engine::OptionId>,
    /// Root effect being (re-)resolved. A native leaf is just one node.
    pub effect: card_dsl::dsl::Effect,
    /// [`EvalContext`](crate::engine::EvalContext)`.controller` ingredient.
    pub controller: InvestigatorId,
    /// [`EvalContext`](crate::engine::EvalContext)`.source` ingredient
    /// (`None` for scenario / forced effects with no originating instance).
    pub source: Option<CardInstanceId>,
}

impl Continuation {
    /// The window payload if this frame is a [`Continuation::Resolution`].
    #[must_use]
    pub fn as_resolution(&self) -> Option<&ResolutionFrame> {
        match self {
            Continuation::Resolution(w) => Some(w),
            Continuation::SkillTest | Continuation::Choice(_) => None,
        }
    }

    /// Mutable counterpart to [`Self::as_resolution`].
    pub fn as_resolution_mut(&mut self) -> Option<&mut ResolutionFrame> {
        match self {
            Continuation::Resolution(w) => Some(w),
            Continuation::SkillTest | Continuation::Choice(_) => None,
        }
    }
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
    /// Effect to run **on failure** after the chaos token resolves,
    /// with the failure margin available via
    /// [`EvalContext::failed_by`](crate::engine::evaluator::EvalContext::failed_by).
    /// Carried by treachery-Revelation tests (`Effect::SkillTest`);
    /// `None` for action tests, which have only the success-side
    /// [`follow_up`](Self::follow_up). Orthogonal to `follow_up` —
    /// success and margin-keyed-failure are separate axes.
    pub on_fail: Option<card_dsl::dsl::Effect>,
    /// Effect to run **on success** after the chaos token resolves (the
    /// success-side mirror of [`on_fail`](Self::on_fail)). Carried by
    /// `Effect::SkillTest` with a success branch — Frozen in Fear 01164's
    /// end-of-turn willpower test discards the card on success. `None` for
    /// action tests and failure-only card tests.
    pub on_success: Option<card_dsl::dsl::Effect>,
    /// The firing card instance, threaded so the `on_success` / `on_fail`
    /// eval-contexts can resolve [`Effect::DiscardSelf`](card_dsl::dsl::Effect::DiscardSelf) across the
    /// suspend/resume boundary. `None` for action tests and effects with
    /// no originating instance.
    pub source: Option<CardInstanceId>,
    /// Where the resolution driver should resume on the next call to
    /// `drive_skill_test`. Initialized to
    /// [`FinishContinuation::AwaitingCommit`] at
    /// `start_skill_test`; advanced in lock-step as the resolution
    /// sequence runs. Post-commit variants carry the test's outcome
    /// as a `succeeded` payload (see [`FinishContinuation`]) so the
    /// invariant "outcome is known iff the test is past the commit
    /// window" is structural.
    pub continuation: FinishContinuation,
    /// A flat modifier applied to the test total, snapshotted by the
    /// effect that initiated the test (`Effect::Fight`'s combat
    /// modifier). `0` for player-action tests, which take their
    /// modifiers from cards in play. Distinct from constant/pending
    /// modifiers — this is the one-shot "+N for this attack" a weapon
    /// grants.
    pub test_modifier: i8,
    /// Bonus damage added to this attack, accumulated at commit time by
    /// [`Effect::BoostAttackDamage`](crate::dsl::Effect::BoostAttackDamage)
    /// (Vicious Blow 01025). Read **only** by the `Fight` follow-up, which
    /// deals `1 + extra_damage + bonus_attack_damage` on success — so it
    /// is inert for non-Fight tests. `0` for every test that no
    /// commit-time attack buff touches (regression-safe).
    pub bonus_attack_damage: u8,
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
    /// iteration fires a Retaliate attack if the test was a failed Fight
    /// against a ready retaliate enemy (Rules Reference p.18 — "after
    /// applying all results for that skill test"), then advances to
    /// teardown.
    PostRetaliate {
        /// The chaos-token resolution's success determination — Retaliate
        /// fires only on failure.
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
        /// Bonus damage beyond the base 1 (weapons). `0` for a basic Fight.
        extra_damage: u8,
    },
    /// On success, disengage the named enemy from the investigator and
    /// exhaust it. Used by [`Evade`](crate::action::PlayerAction::Evade).
    Evade {
        /// The enemy the Evade action targeted.
        enemy: EnemyId,
    },
}

/// Which investigators may submit Fast `PlayCard` / `ActivateAbility`
/// actions while a [`ResolutionFrame`] is the top of the window stack.
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
pub struct ResolutionFrame {
    /// Candidates in resolution order. For a reaction window: active
    /// investigator's matching reactions first, then others in turn
    /// order. For the forced run: the simultaneous forced abilities the
    /// lead orders. Empty is permitted — framework windows opened for
    /// phase/timing reasons gate Fast actions with no pending candidates.
    pub pending_triggers: Vec<ResolutionCandidate>,
    /// What this resolution run *is*: either a reaction / fast / framework
    /// [`Window`](ResolutionKind::Window) (carrying its kind + Fast-action
    /// scope), or the mandatory **forced run**
    /// ([`Forced`](ResolutionKind::Forced), Axis-B T5b / #213) — which
    /// cannot be skipped, admits no Fast plays, and on close resumes the
    /// framework flow it suspended via its [`ForcedContinuation`].
    pub kind: ResolutionKind,
}

/// What a [`ResolutionFrame`] is resolving: a reaction/fast/framework
/// window, or the mandatory forced run.
///
/// The two arms differ in close behavior. A [`Window`](Self::Window) runs
/// its per-kind window continuation (or simply pops, for after-event
/// reaction windows). The [`Forced`](Self::Forced) run instead resumes the
/// framework flow that opened it — see [`ForcedContinuation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionKind {
    /// A reaction / fast / framework window: skippable, admits Fast plays
    /// from its [`fast_actors`](WindowBinding::fast_actors) scope, and runs
    /// a per-kind continuation on close.
    Window(WindowBinding),
    /// The forced run (#213): mandatory, no Fast plays, and on close
    /// resumes the framework flow named by the [`ForcedContinuation`].
    Forced(ForcedContinuation),
}

/// How a [`Forced`](ResolutionKind::Forced) run resumes the framework flow
/// it suspended when 2+ simultaneous forced abilities forced a lead-ordered
/// choice (#213).
///
/// Most emit sites are *terminal*: nothing in the framework runs after the
/// forced abilities resolve, so the run closes to [`Terminal`](Self::Terminal)
/// and control returns to the caller. Sites with framework work *after* the
/// emit (e.g. the upkeep step continues after `RoundEnded`) carry a dedicated
/// variant naming that tail, so the suspended flow is resumed exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ForcedContinuation {
    /// No framework work follows the forced run; closing returns control to
    /// the emit site's caller.
    Terminal,
    /// Resume the upkeep step's tail after `RoundEnded`'s forced abilities
    /// resolve: open the act round-end advance window, then step the phase.
    UpkeepAfterRoundEnded,
    /// Resume the end-of-turn step (RR p.24 2.2.2) after a turn-ending
    /// investigator's `EndOfTurn` forced abilities resolve: rotate to the
    /// next active investigator, or end the Investigation phase.
    EndOfTurnAfterForced {
        /// The investigator whose turn ended.
        investigator: InvestigatorId,
    },
}

/// The window-specific part of a [`ResolutionFrame`]: which kind of window
/// is open and which investigators may submit Fast actions while it is the
/// top of the stack. Absent for the forced run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBinding {
    /// What kind of window is open; carries the IDs the triggering event
    /// named (defeated enemy + attacker, phase transition, etc.) so
    /// pending triggers' effects can resolve against the same payload.
    pub kind: WindowKind,
    /// Which investigators may submit Fast `PlayCard` / `ActivateAbility`
    /// actions while this window is the top of the stack.
    pub fast_actors: FastActorScope,
}

impl ResolutionFrame {
    /// Construct an empty [`ResolutionFrame`] (no pending triggers) for the
    /// given `kind` and `fast_actors` scope.
    ///
    /// Provided so integration tests outside the crate (where the
    /// `#[non_exhaustive]` attribute blocks struct-literal construction)
    /// can inject a window directly onto
    /// [`GameState::open_windows`] for stack-shape regression tests.
    #[must_use]
    pub fn new_empty(kind: WindowKind, fast_actors: FastActorScope) -> Self {
        Self {
            pending_triggers: Vec::new(),
            kind: ResolutionKind::Window(WindowBinding { kind, fast_actors }),
        }
    }

    /// The [`WindowKind`] if this frame is a window; `None` for the forced
    /// run.
    #[must_use]
    pub fn kind(&self) -> Option<WindowKind> {
        match &self.kind {
            ResolutionKind::Window(w) => Some(w.kind),
            ResolutionKind::Forced(_) => None,
        }
    }

    /// The Fast-action scope if this frame is a window; `None` for the
    /// forced run (no Fast plays).
    #[must_use]
    pub fn fast_actors(&self) -> Option<&FastActorScope> {
        match &self.kind {
            ResolutionKind::Window(w) => Some(&w.fast_actors),
            ResolutionKind::Forced(_) => None,
        }
    }

    /// The [`ForcedContinuation`] if this is the forced run; `None` for a
    /// window. Read on close to resume the suspended framework flow.
    #[must_use]
    pub fn forced_continuation(&self) -> Option<ForcedContinuation> {
        match &self.kind {
            ResolutionKind::Forced(c) => Some(*c),
            ResolutionKind::Window(_) => None,
        }
    }

    /// Whether this is the forced-resolution run (mandatory, no window).
    /// The complement of being a reaction / fast / framework window.
    #[must_use]
    pub fn is_forced(&self) -> bool {
        matches!(self.kind, ResolutionKind::Forced(_))
    }
}

/// Discriminant of an open `ResolutionFrame`.
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
    /// An enemy attack placed damage on a controlled asset (soak). Opens
    /// after placement so the soaked asset's `EnemyAttackDamagedSelf`
    /// reaction (Guard Dog 01021) can fire. `asset` is the soaked
    /// instance, `enemy` the attacker (threaded into the reaction's
    /// `EvalContext.attacking_enemy`), `controller` the asset's owner.
    /// (C5b #237.)
    AfterEnemyAttackDamagedAsset {
        /// The card instance that absorbed the damage.
        asset: CardInstanceId,
        /// The enemy whose attack caused the damage.
        enemy: EnemyId,
        /// The investigator who controls the soaked asset.
        controller: InvestigatorId,
    },
    /// Fires after an investigator successfully investigated. Pairs with
    /// [`EventPattern::SuccessfullyInvestigated`](crate::dsl::EventPattern::SuccessfullyInvestigated)
    /// with [`EventTiming::After`](crate::dsl::EventTiming::After). Queued
    /// from the Investigate skill-test follow-up (success-only by
    /// construction). `investigator` is who investigated; a reaction only
    /// fires for its own controller's investigation ("after **you**
    /// investigate" — Dr. Milan Christopher 01033). (C6a #241.)
    AfterSuccessfulInvestigate {
        /// The investigator who successfully investigated.
        investigator: InvestigatorId,
    },
    /// Before-timing window: an enemy is about to attack `investigator` (RR
    /// p.25 step 3.3). Opens *before* damage is dealt so a co-located cancel
    /// reaction (Dodge 01023) can cancel the attack. (Axis D #336.)
    BeforeEnemyAttack {
        /// The attacking enemy.
        enemy: EnemyId,
        /// The investigator being attacked.
        investigator: InvestigatorId,
    },
    /// Before-timing window: `investigator` is about to discover `count`
    /// clues at `location`. Opens *before* the discovery so a replacement
    /// reaction (Cover Up 01007) can discard-instead and cancel it. (Axis D
    /// #336; migrated from the C5a `clue_interrupt` seam.)
    BeforeDiscoverClues {
        /// The discovering investigator.
        investigator: InvestigatorId,
        /// The location the clues would come from.
        location: LocationId,
        /// The number of clues that would be discovered.
        count: u8,
    },
    /// Fires after a card entered play, scanning only the entered instance's
    /// own `EnteredPlay` reactions (Research Librarian 01032). Pairs with
    /// [`EventPattern::EnteredPlay`](crate::dsl::EventPattern::EnteredPlay) /
    /// [`EventTiming::After`](crate::dsl::EventTiming::After). `instance` is the
    /// entered card; `controller` its owner.
    AfterEnteredPlay {
        /// The card instance that just entered play (self-binding scope).
        instance: CardInstanceId,
        /// The investigator who controls it.
        controller: InvestigatorId,
    },
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

/// Where a [`ResolutionCandidate`] comes from — which decides how it
/// *resolves* when picked.
///
/// `InPlay` and `Board` candidates **fire an ability's effect**; a `Hand`
/// candidate (Axis C, #335) is a Fast event **played** from hand (RR
/// Appendix I — `CardPlayed`, run the matched ability's effect, discard),
/// not fired in place. Replacing the former `source: Option<CardInstanceId>`
/// with this enum lets one `pending_triggers` list carry hand events
/// alongside in-play reactions: `None` (board) and "from hand" are distinct
/// origins, so a bare `Option` could not tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CandidateSource {
    /// An ability on an in-play / threat-area instance (reaction trigger,
    /// weapon, …). The instance id drives `Effect::DiscardSelf`, usage-limit
    /// bumping, and the soak self-binding.
    InPlay(CardInstanceId),
    /// A scenario board card (act / agenda) — no instance; fires by `code`.
    Board,
    /// A Fast event in the controller's hand (Axis C) — *played* rather than
    /// fired. No instance until it would enter play (events never do).
    Hand,
}

impl CandidateSource {
    /// The firing in-play instance, if any — `Some` for [`InPlay`](Self::InPlay),
    /// `None` for [`Board`](Self::Board) (scenario card) and [`Hand`](Self::Hand)
    /// (event not yet in play). Feeds
    /// [`EvalContext::for_controller_with_optional_source`](crate::engine::EvalContext::for_controller_with_optional_source).
    #[must_use]
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            CandidateSource::InPlay(id) => Some(id),
            CandidateSource::Board | CandidateSource::Hand => None,
        }
    }
}

/// A single pending ability/play waiting to resolve in a
/// [`Continuation::Resolution`] frame.
///
/// The **unified candidate** for the forced run, a reaction window's in-play
/// triggers, *and* (Axis C) a Fast event playable from hand: abilities resolve
/// by `code` (registry lookup), so the same shape serves in-play instances,
/// scenario board cards (act / agenda), and hand events. How a picked
/// candidate resolves is decided by its [`source`](Self::source)
/// ([`CandidateSource`]). Whether a candidate is mandatory vs. optional is a
/// property of the *frame*, not the candidate — forced and reaction are
/// separate resolution runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResolutionCandidate {
    /// Printed code of the card whose ability fires (or which is played, for
    /// a [`CandidateSource::Hand`] event). Abilities are looked up by code.
    pub code: CardCode,
    /// The investigator the effect resolves under (controller / player).
    pub controller: InvestigatorId,
    /// Zero-based index into the card's
    /// [`abilities`](crate::dsl::Ability) vec — which ability fires / runs.
    pub ability_index: u8,
    /// Where the candidate comes from, deciding how it resolves — see
    /// [`CandidateSource`].
    pub source: CandidateSource,
}

impl ResolutionCandidate {
    /// Construct a [`ResolutionCandidate`]. Provided so integration tests
    /// outside the crate (where `#[non_exhaustive]` blocks struct-literal
    /// construction) can build a window's pending triggers directly — the same
    /// rationale as [`ResolutionFrame::new_empty`].
    #[must_use]
    pub fn new(
        code: CardCode,
        controller: InvestigatorId,
        ability_index: u8,
        source: CandidateSource,
    ) -> Self {
        Self {
            code,
            controller,
            ability_index,
            source,
        }
    }
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
    /// The topmost open window that has unresolved candidates, if any. Used
    /// by the dispatcher's "is reaction work pending?" guards. A candidate is
    /// an in-play trigger *or* a hand Fast-event play (Axis C) — both ride
    /// `pending_triggers`. Pure Fast-gating framework windows (empty
    /// `pending_triggers`) are skipped — they don't block dispatch.
    #[must_use]
    pub fn top_reaction_window(&self) -> Option<&ResolutionFrame> {
        self.windows()
            .rev()
            .find(|w| !w.pending_triggers.is_empty())
    }

    /// Mutable counterpart to `top_reaction_window`. Same skip rule
    /// applies: windows with empty `pending_triggers` are skipped —
    /// phase-gate-only windows are not exposed as reaction-work.
    pub fn top_reaction_window_mut(&mut self) -> Option<&mut ResolutionFrame> {
        self.continuations
            .iter_mut()
            .rev()
            .filter_map(Continuation::as_resolution_mut)
            .find(|w| !w.pending_triggers.is_empty())
    }

    /// Iterator over the open windows on the continuation stack, in stack
    /// order (bottom to top). The windows are `Continuation::Resolution`
    /// frames; non-window frames (Task 4+) are skipped.
    fn windows(&self) -> impl DoubleEndedIterator<Item = &ResolutionFrame> {
        self.continuations
            .iter()
            .filter_map(Continuation::as_resolution)
    }

    /// The open windows as a `Vec` of references, in stack order. Read
    /// accessor for callers (and tests) that inspect the window stack the
    /// way they used to read the former `open_windows` field.
    #[must_use]
    pub fn open_windows(&self) -> Vec<&ResolutionFrame> {
        self.windows().collect()
    }

    /// The topmost open window regardless of pending triggers (the former
    /// `open_windows.last()`), e.g. for the Fast-play `permissive_window`
    /// timing gate. Distinct from [`Self::top_reaction_window`], which
    /// skips empty-`pending_triggers` (pure-Fast) windows.
    #[must_use]
    pub fn top_window(&self) -> Option<&ResolutionFrame> {
        self.windows().next_back()
    }

    /// Index into [`Self::open_windows`] of the topmost window with
    /// non-empty `pending_triggers`, matching the window that
    /// [`Self::top_reaction_window`] / [`Self::top_reaction_window_mut`]
    /// resolve to.
    ///
    /// Callers driving the reaction window pass this index to
    /// `close_reaction_window_at` so the close path removes the same
    /// entry the driver was operating on, rather than blindly popping
    /// the top of the stack — a `PlayerWindow` gate with empty
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
        self.continuations.iter().rposition(|c| {
            c.as_resolution()
                .is_some_and(|w| !w.pending_triggers.is_empty())
        })
    }

    /// Build a [`Location`] from its card `metadata`, minting a fresh id.
    /// Panics if `metadata` is not a `Location` card (a build-time
    /// invariant — scenarios hand their own location cards).
    fn location_from_metadata(&mut self, metadata: &CardMetadata) -> Location {
        let (shroud, printed_clues) = match &metadata.kind {
            CardKind::Location {
                shroud,
                printed_clues,
                ..
            } => (*shroud, *printed_clues),
            other => panic!(
                "add_location: card {} is not a Location ({other:?})",
                metadata.code
            ),
        };
        let id = self.location_ids.mint();
        Location {
            id,
            code: CardCode::new(metadata.code.clone()),
            name: metadata.name.clone(),
            shroud,
            clues: 0,
            revealed: false,
            printed_clues,
            connections: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// Add a location **into play** from its card metadata, returning the
    /// minted [`LocationId`]. The id is deterministic (construction order),
    /// so scenarios never hand-pick id literals.
    pub fn add_location(&mut self, metadata: &CardMetadata) -> LocationId {
        let loc = self.location_from_metadata(metadata);
        let id = loc.id;
        self.locations.insert(id, loc);
        id
    }

    /// Add a location to the **set-aside** (out-of-play) zone from its card
    /// metadata, returning the minted [`LocationId`]. Card effects (e.g. The
    /// Gathering's Act-1 reverse) later move it into play.
    pub fn add_set_aside_location(&mut self, metadata: &CardMetadata) -> LocationId {
        let loc = self.location_from_metadata(metadata);
        let id = loc.id;
        self.set_aside_locations.push(loc);
        id
    }

    /// Add an enemy to the **set-aside** (out-of-play) zone, recording its
    /// printed code only. Unlike set-aside locations (fully built here),
    /// an enemy's stats — notably per-investigator health — depend on the
    /// in-game investigator count, which isn't known at `setup()`; so the
    /// `Enemy` is minted from the corpus when a card effect brings it into
    /// play (see [`spawn_set_aside_enemy`](crate::engine::spawn_set_aside_enemy)).
    /// Panics on non-Enemy metadata — a setup-time invariant.
    pub fn add_set_aside_enemy(&mut self, metadata: &CardMetadata) {
        assert!(
            matches!(metadata.kind, CardKind::Enemy { .. }),
            "add_set_aside_enemy: card {} is not an Enemy ({:?})",
            metadata.code,
            metadata.kind,
        );
        self.set_aside_enemies
            .push(CardCode::new(metadata.code.clone()));
    }

    /// Find a location by id across both the in-play and set-aside zones.
    fn location_mut(&mut self, id: LocationId) -> Option<&mut Location> {
        if let Some(loc) = self.locations.get_mut(&id) {
            return Some(loc);
        }
        self.set_aside_locations.iter_mut().find(|l| l.id == id)
    }

    /// Wire a **bidirectional** connection between two locations (each gains
    /// the other in its `connections`). Resolves both ids across the in-play
    /// and set-aside zones. `expect`s each to exist — a build-time invariant
    /// (callers connect freshly-minted ids).
    pub fn connect(&mut self, a: LocationId, b: LocationId) {
        self.location_mut(a)
            .unwrap_or_else(|| panic!("connect: location {a:?} not found"))
            .connections
            .push(b);
        self.location_mut(b)
            .unwrap_or_else(|| panic!("connect: location {b:?} not found"))
            .connections
            .push(a);
    }
}

#[cfg(test)]
mod open_window_tests {
    use super::*;

    #[test]
    fn open_window_serde_roundtrip() {
        let window = ResolutionFrame {
            pending_triggers: Vec::new(),
            kind: ResolutionKind::Window(WindowBinding {
                kind: WindowKind::AfterEnemyDefeated {
                    enemy: EnemyId(7),
                    by: Some(InvestigatorId(1)),
                },
                fast_actors: FastActorScope::Any,
            }),
        };
        let json = serde_json::to_string(&window).expect("serialize");
        let back: ResolutionFrame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, window);
    }

    #[test]
    fn player_window_kind_serde_roundtrip() {
        let kind = WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws);
        let json = serde_json::to_string(&kind).expect("serialize");
        let back: WindowKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn hand_candidate_serde_round_trips() {
        // A Fast event playable from hand (Axis C) rides ResolutionCandidate
        // with a `Hand` source — distinct from a board card's `None`/`Board`.
        let candidate = ResolutionCandidate {
            code: CardCode::new("01022"),
            controller: InvestigatorId(1),
            ability_index: 0,
            source: CandidateSource::Hand,
        };
        let json = serde_json::to_string(&candidate).expect("serialize");
        let back: ResolutionCandidate = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, candidate);
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
mod location_id_counter_tests {
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_starts_location_ids_at_zero() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.location_ids.peek(), 0);
    }

    #[test]
    fn location_ids_round_trip_through_serde() {
        use crate::state::Counter;
        let mut state = GameStateBuilder::new().build();
        state.location_ids = Counter::at(7);
        let json = serde_json::to_string(&state).expect("serialize");
        let back: crate::state::GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.location_ids.peek(), 7);
    }
}

#[cfg(test)]
mod continuation_stack_tests {
    use super::*;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn continuations_default_empty_and_absent_field_loads() {
        let s = GameStateBuilder::new().build();
        assert!(s.continuations.is_empty());

        // A pre-field serialized state (no `continuations` key) still loads,
        // defaulting to an empty stack (`#[serde(default)]`).
        let mut v = serde_json::to_value(&s).expect("serialize");
        v.as_object_mut()
            .expect("state serializes to a JSON object")
            .remove("continuations");
        let back: GameState = serde_json::from_value(v).expect("deserialize without the field");
        assert!(back.continuations.is_empty());
    }

    #[test]
    fn open_window_lives_on_the_continuation_stack_as_a_resolution_frame() {
        // Axis-B T3: a window is a `Continuation::Resolution` frame on the
        // one stack — there is no separate `open_windows` Vec.
        let state = GameStateBuilder::new()
            .with_open_window(
                WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
                FastActorScope::Any,
            )
            .build();
        assert_eq!(state.continuations.len(), 1);
        assert!(matches!(
            state.continuations[0],
            Continuation::Resolution(_)
        ));
        // The read accessor surfaces it as the former `open_windows` view.
        assert_eq!(state.open_windows().len(), 1);
        assert_eq!(
            state.open_windows()[0].kind(),
            Some(WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws)),
        );
    }
}

#[cfg(test)]
mod id_counter_tests {
    use super::*;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_starts_enemy_ids_at_zero() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.enemy_ids.peek(), 0);
    }

    #[test]
    fn enemy_ids_round_trip_through_serde() {
        let mut state = GameStateBuilder::new().build();
        state.enemy_ids = Counter::at(42);
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.enemy_ids.peek(), 42);
    }

    #[test]
    fn each_id_counter_mints_independently() {
        let mut state = GameStateBuilder::new().build();
        assert_eq!(state.card_instance_ids.mint(), CardInstanceId(0));
        assert_eq!(state.card_instance_ids.mint(), CardInstanceId(1));
        // Each id type draws from its own counter — minting one doesn't
        // disturb the others.
        assert_eq!(state.enemy_ids.mint(), EnemyId(0));
        assert_eq!(state.location_ids.mint(), LocationId(0));
        assert_eq!(state.enemy_ids.mint(), EnemyId(1));
        assert_eq!(state.card_instance_ids.peek(), 2);
        assert_eq!(state.enemy_ids.peek(), 2);
        assert_eq!(state.location_ids.peek(), 1);
    }
}

#[cfg(test)]
mod mythos_draw_pending_tests {
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_default_has_no_mythos_draw_pending() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.mythos_draw_pending, None);
    }
}

#[cfg(test)]
mod enemy_attack_pending_tests {
    use super::*;
    use crate::state::InvestigatorId;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_default_has_no_enemy_attack_pending() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.enemy_attack_pending, None);
    }

    #[test]
    fn enemy_attack_pending_round_trips_through_serde() {
        let mut state = GameStateBuilder::new().build();
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
    use crate::test_support::GameStateBuilder;

    #[test]
    fn encounter_deck_and_discard_serde_roundtrip() {
        let mut state = GameStateBuilder::new().build();
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
        let state = GameStateBuilder::new().build();
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
mod act_agenda_code_tests {
    use super::*;

    #[test]
    fn act_and_agenda_carry_card_code() {
        let act = Act {
            code: CardCode("01108".into()),
            clue_threshold: 2,
            resolution: None,
            round_end_advance: None,
        };
        let agenda = Agenda {
            code: CardCode("01105".into()),
            doom_threshold: 3,
            resolution: None,
        };
        assert_eq!(act.code, CardCode("01108".into()));
        assert_eq!(agenda.code, CardCode("01105".into()));
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

#[cfg(test)]
mod add_location_tests {
    use crate::card_data::{CardKind, CardMetadata, ClueValue};
    use crate::test_support::GameStateBuilder;

    fn location_meta(code: &str, name: &str, shroud: u8, clues: u8) -> CardMetadata {
        CardMetadata {
            code: code.to_string(),
            name: name.to_string(),
            traits: vec![],
            text: None,
            pack_code: "core".to_string(),
            kind: CardKind::Location {
                shroud,
                printed_clues: ClueValue::PerInvestigator(clues),
                victory: None,
            },
        }
    }

    #[test]
    fn add_location_mints_sequential_ids_and_extracts_metadata() {
        let mut state = GameStateBuilder::new().build();
        let a = state.add_location(&location_meta("01111", "Study", 2, 2));
        let b = state.add_location(&location_meta("01112", "Hallway", 1, 0));
        assert_ne!(a, b, "ids are distinct");
        let study = &state.locations[&a];
        assert_eq!(study.code.as_str(), "01111");
        assert_eq!(study.name, "Study");
        assert_eq!(study.shroud, 2);
        assert_eq!(study.clues, 0, "enters unrevealed with no clues");
        assert!(!study.revealed);
        assert_eq!(
            study.printed_clues,
            crate::card_data::ClueValue::PerInvestigator(2)
        );
        assert!(study.connections.is_empty());
        assert_eq!(state.location_ids.peek(), 2, "counter advanced twice");
    }

    #[test]
    fn add_set_aside_location_goes_to_the_set_aside_zone() {
        let mut state = GameStateBuilder::new().build();
        let id = state.add_set_aside_location(&location_meta("01113", "Attic", 1, 2));
        assert!(!state.locations.contains_key(&id), "not in play");
        assert_eq!(state.set_aside_locations.len(), 1);
        assert_eq!(state.set_aside_locations[0].id, id);
        assert_eq!(state.set_aside_locations[0].code.as_str(), "01113");
    }

    fn enemy_meta(code: &str, name: &str) -> CardMetadata {
        use crate::card_data::Prey;
        CardMetadata {
            code: code.to_string(),
            name: name.to_string(),
            traits: vec![],
            text: None,
            pack_code: "core".to_string(),
            kind: CardKind::Enemy {
                fight: 1,
                evade: 1,
                damage: 0,
                horror: 0,
                health: None,
                victory: None,
                spawn: None,
                surge: false,
                peril: false,
                hunter: false,
                retaliate: false,
                prey: Prey::Default,
                quantity: 1,
            },
        }
    }

    #[test]
    fn add_set_aside_enemy_records_the_code() {
        let mut state = GameStateBuilder::new().build();
        state.add_set_aside_enemy(&enemy_meta("01116", "Ghoul Priest"));
        assert_eq!(
            state.set_aside_enemies,
            vec![crate::state::CardCode::new("01116")],
            "set-aside enemies record only the code (stats minted at spawn)",
        );
    }

    #[test]
    #[should_panic(expected = "not an Enemy")]
    fn add_set_aside_enemy_panics_on_non_enemy_metadata() {
        let mut state = GameStateBuilder::new().build();
        state.add_set_aside_enemy(&location_meta("01113", "Attic", 1, 2));
    }

    #[test]
    #[should_panic(expected = "not a Location")]
    fn add_location_panics_on_non_location_metadata() {
        let mut state = GameStateBuilder::new().build();
        let meta = CardMetadata {
            code: "01108".to_string(),
            name: "Trapped".to_string(),
            traits: vec![],
            text: None,
            pack_code: "core".to_string(),
            kind: CardKind::Act {
                clue_threshold: Some(2),
                victory: None,
            },
        };
        state.add_location(&meta);
    }
}

#[cfg(test)]
mod connect_tests {
    use crate::state::{CardCode, Location, LocationId};
    use crate::test_support::GameStateBuilder;

    #[test]
    fn connect_wires_both_directions() {
        let mut state = GameStateBuilder::new()
            .with_location(Location::new(
                LocationId(1),
                CardCode("a".into()),
                "A",
                1,
                0,
            ))
            .with_location(Location::new(
                LocationId(2),
                CardCode("b".into()),
                "B",
                1,
                0,
            ))
            .build();
        state.connect(LocationId(1), LocationId(2));
        assert_eq!(
            state.locations[&LocationId(1)].connections,
            vec![LocationId(2)]
        );
        assert_eq!(
            state.locations[&LocationId(2)].connections,
            vec![LocationId(1)]
        );
    }

    #[test]
    fn connect_resolves_set_aside_locations() {
        // Both endpoints live in the set-aside zone (The Gathering wires
        // its board there before Act 1 brings it into play).
        let mut state = GameStateBuilder::new().build();
        state.set_aside_locations.push(Location::new(
            LocationId(2),
            CardCode("hub".into()),
            "Hub",
            1,
            0,
        ));
        state.set_aside_locations.push(Location::new(
            LocationId(3),
            CardCode("spoke".into()),
            "Spoke",
            1,
            0,
        ));
        state.connect(LocationId(2), LocationId(3));
        let hub = state
            .set_aside_locations
            .iter()
            .find(|l| l.id == LocationId(2))
            .unwrap();
        let spoke = state
            .set_aside_locations
            .iter()
            .find(|l| l.id == LocationId(3))
            .unwrap();
        assert_eq!(hub.connections, vec![LocationId(3)]);
        assert_eq!(spoke.connections, vec![LocationId(2)]);
    }
}

#[cfg(test)]
mod starting_location_tests {
    use super::*;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_starting_location_defaults_to_none_and_roundtrips() {
        let mut state = GameStateBuilder::new().build();
        assert_eq!(state.starting_location, None, "default must be None");

        state.starting_location = Some(crate::state::LocationId(7));
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.starting_location, Some(crate::state::LocationId(7)));
    }
}
