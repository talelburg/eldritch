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
    // The setup mulligan loop now lives on its `Continuation::Mulligan`
    // frame (#348); read the prompted investigator via
    // [`Self::current_mulligan`]. The former `mulligan_pending:
    // Option<InvestigatorId>` cursor is removed — the continuation stack is the
    // single source of truth (mirroring the `in_flight_skill_test` fold).
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
    // The in-flight skill test now lives on its `Continuation::SkillTest(_)`
    // frame (#348); read it via [`Self::current_skill_test`]. The former
    // `in_flight_skill_test: Option<InFlightSkillTest>` field is removed —
    // the continuation stack is the single source of truth.
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
    /// `TimingPointWindow` / `FastWindow` frames (the former `open_windows` Vec,
    /// absorbed into the one stack). `#[serde(default)]` so pre-field
    /// states still load. Inspect windows via [`Self::open_windows`] /
    /// [`Self::top_window`].
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
    // The Mythos step-1.4 encounter-draw loop now lives on its
    // `Continuation::EncounterDraw` frame (#348); read the prompted drawer via
    // [`Self::current_encounter_drawer`]. The former `mythos_draw_pending:
    // Option<InvestigatorId>` cursor is removed — the continuation stack is the
    // single source of truth (mirroring the `mulligan_pending` fold).
    /// Set by [`Effect::Cancel`](crate::dsl::Effect::Cancel) while a
    /// Before-timing reaction window resolves; read-and-cleared by the emit
    /// site (the enemy-attack loop, `discover_clue`) after the window closes,
    /// to skip the prevented impact (Axis D #336). A bool suffices because
    /// Before-windows do not nest in scope — exactly one cancellable impact is
    /// ever in flight. TODO(#367): typed marker once Before-windows can nest.
    #[serde(default)]
    pub pending_cancellation: bool,
    // The former `pending_revelation_discard: Option<CardCode>` side-channel is
    // removed (#380): a drawn treachery's disposal now rides a
    // `Continuation::EncounterCard` frame whose framework teardown discards it
    // once the Revelation's whole sub-resolution completes — covering a
    // Revelation that suspends into a choice, not just a skill test.
    /// An event card mid-play: it has left hand ("commences being played",
    /// RR Appendix I step 3) but is not yet in discard. The apply loop
    /// flushes it to the owner's discard pile on `Done` (step 4: the event is
    /// placed in discard "simultaneously with the completion" of its effect),
    /// so an `OnPlay` effect that suspends — Dynamite Blast 01024's location
    /// choice — discards the event when it resumes rather than stranding it in
    /// hand. The player-event analogue of the treachery disposal that #380
    /// moved onto the [`EncounterCard`](Continuation::EncounterCard) frame
    /// (a sibling side-channel, folded similarly in a future cycle). `None`
    /// outside an in-flight event play.
    #[serde(default)]
    pub pending_played_event: Option<(InvestigatorId, CardCode)>,
    /// Active round-scoped skill substitutions (Mind over Matter 01036).
    /// While present, the owning investigator may make a `for_skills` test as
    /// a `use_skill` test instead (offered at test initiation). Cleared at the
    /// round boundary ("until the end of the round").
    #[serde(default)]
    pub skill_substitutions: Vec<SkillSubstitution>,
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
    /// Attack of opportunity (`drive_aoo`).
    AttackOfOpportunity,
    /// Retaliate attack from a failed Fight (`drive_retaliate`, RR p.18).
    Retaliate,
}

/// Which point in the per-attacker sequence a parked enemy-attack loop
/// suspended at (Axis D #336).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackLoopStage {
    /// Suspended on the `BeforeEnemyAttack` cancel window, *before* the head
    /// attacker dealt damage. Resume reads `pending_cancellation`, then deals
    /// (or skips) and exhausts the head attacker.
    BeforeAttack,
    /// Suspended on the `AfterEnemyAttackDamagedAsset` soak window, *after*
    /// the head attacker dealt + exhausted. Resume drains the rest (the
    /// pre-Axis-D behavior).
    AfterSoak,
    /// Suspended on the player's attack-order `PickSingle` (#143/K4): 2+
    /// attackers remain and none has dealt this iteration. The `AttackLoop`
    /// frame is the **top** frame (no reaction window above it) and *is* the
    /// prompt. Resume reorders `remaining_attackers` to put the picked enemy at
    /// the head, deals it, then continues. Unlike the window stages — which park
    /// *beneath* a reaction window and resume on window-close via
    /// [`resume_enemy_attack`](crate::engine) — this stage resumes on
    /// `ResolveInput` via `resume_attack_order_pick`.
    PickOrder,
}

/// A computed damage/horror distribution for one source of harm (C5b #237).
///
/// How much of the harm's damage and horror lands on the defending investigator
/// versus each soak-bearing asset. Built by `assign_attack` (soak-first) or the
/// interactive per-point distribution (#44/K5b); placed simultaneously by
/// `place_assignment`, per Rules Reference page 7's "Apply Damage/Horror" clause.
/// Lives here (not in `engine`) because an in-progress one is parked on a
/// [`Continuation::DamageAssignment`] frame.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Damage absorbed by the investigator.
    pub investigator_damage: u8,
    /// Horror absorbed by the investigator.
    pub investigator_horror: u8,
    /// instance → damage soaked onto that asset.
    pub asset_damage: std::collections::BTreeMap<CardInstanceId, u8>,
    /// instance → horror soaked onto that asset.
    pub asset_horror: std::collections::BTreeMap<CardInstanceId, u8>,
}

/// How a [`Continuation::DamageAssignment`] resumes once the player has finished
/// distributing the harm across soakers and themselves (#44/K5b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageSource {
    /// An enemy attack: after placement, queue soak reaction windows for the
    /// damaged survivors, exhaust the attacker (enemy phase), and continue the
    /// attack loop over `remaining_attackers`.
    EnemyAttack {
        /// The attacking enemy (for the soak window + exhaust).
        enemy: EnemyId,
        /// Attackers not yet resolved, in resolution order (head already removed).
        remaining_attackers: Vec<EnemyId>,
        /// Which loop drives this attack.
        attack_source: EnemyAttackSource,
    },
    /// A card/treachery `Effect::Deal` (K5b-2): after placing the drained point,
    /// resume the parked effect walk so any remaining iterations run.
    Effect,
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

/// A frame on the [`GameState::continuations`] suspend/resume stack
/// (umbrella §1 / Axis-B): a typed resume point, not a closure, so it
/// serializes for replay/persistence like every other state field.
///
/// Open windows live here as [`TimingPointWindow`](Self::TimingPointWindow)
/// (event windows + the #213 forced run) and [`FastWindow`](Self::FastWindow)
/// (framework player windows). A window is just "paused, the player may act
/// here, resume on act/pass," so it is a continuation frame: this absorbs the
/// former `open_windows` Vec into the one stack (umbrella §1 — no separate
/// window structure to keep in sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Continuation {
    /// An event reaction window or the #213 forced run, keyed by the
    /// [`TimingEvent`](crate::engine::TimingEvent) that opened it (EmitEvent-frame
    /// Slice A, #433). The [`mode`](TimingMode) distinguishes a skippable
    /// reaction window from the mandatory forced run (which carries its
    /// [`ForcedContinuation`]). The `TimingEvent` is referenced in place rather
    /// than relocated — [`Effect`](Self::Effect) already holds a `crate::engine`
    /// type ([`EvalContext`](crate::engine::EvalContext), #345).
    TimingPointWindow {
        /// The timing event that opened this window/run.
        event: crate::engine::TimingEvent,
        /// Reaction window vs. forced run.
        mode: TimingMode,
        /// Candidates in resolution order (lead-ordered for the forced run;
        /// active-investigator-first for a reaction window).
        candidates: Vec<ResolutionCandidate>,
    },
    /// A framework "red-box" player window — a Rules-Reference timing step
    /// that gates Fast actions and runs a per-step continuation on close
    /// (EmitEvent-frame Slice A, #433). The [`FastWindowKind`] discriminant
    /// routes the close continuation (`Phase` → the `*Phase`
    /// anchor's `on_child_pop`; `SkillTest` → the skill-test driver). Carries no
    /// `TimingEvent` — framework windows are not event-driven.
    FastWindow {
        /// Fast-play candidates (hand Fast events admitted at this window).
        /// Usually empty (a pure Fast-gate) — non-empty only for an
        /// Axis-C hand play offered at a framework step.
        candidates: Vec<ResolutionCandidate>,
        /// Which investigators may submit Fast actions here.
        fast_actors: FastActorScope,
        /// The framework step this window gates (and its event-payload kind).
        kind: FastWindowKind,
    },
    /// A skill test is mid-resolution. Carries the in-flight test's data
    /// directly (the former `GameState::in_flight_skill_test` singleton, folded
    /// onto the frame — #348). Pushed at test start; popped when the test fully
    /// resolves (Axis-B T4). At most one is ever on the stack (no nesting today);
    /// [`GameState::current_skill_test`] returns the topmost one.
    SkillTest(InFlightSkillTest),
    /// A suspended Hunter-movement / engagement choice (#128), migrated off the
    /// former `GameState::hunter_move_pending` field (#348). Resumed by
    /// [`resume_hunter_choice`](crate::engine) via `ResolveInput`.
    HunterMove(HunterChoice),
    /// A suspended engagement-on-spawn choice (#128), migrated off the former
    /// `GameState::spawn_engage_pending` field (#348).
    SpawnEngage(SpawnEngagePending),
    /// A suspended upkeep hand-size discard (#111), migrated off the former
    /// `GameState::hand_size_discard_pending` field (#348).
    HandSizeDiscard(HandSizeDiscard),
    /// A suspended act round-end clue-spend window (#275), migrated off the
    /// former `GameState::act_round_end_pending` field (#348).
    ActRoundEnd(ActRoundEndPending),
    /// A skill test paused on its Mind-over-Matter "use X in place of Y?" prompt
    /// at initiation (#322), migrated off the former
    /// `GameState::pending_substitution_prompt` field (#348). Pushed *above* the
    /// `SkillTest` frame, so top-frame dispatch routes it before the commit
    /// window; resumed by [`resume_substitution_choice`](crate::engine).
    SubstitutionPrompt {
        /// The investigator taking the test.
        investigator: InvestigatorId,
    },
    /// The setup mulligan loop (Rules Reference p.27), migrated off the former
    /// `GameState::mulligan_pending` cursor field (#348). `remaining[0]` is the
    /// investigator currently prompted to mulligan; the queue is the Active
    /// investigators in [`turn_order`](GameState::turn_order). Pushed by
    /// `start_scenario`, advanced by `resume_mulligan` as each investigator
    /// submits their `PickMultiple` redraw indices, popped when drained — at
    /// which point setup ends and the Investigation phase begins. While present,
    /// the engine rejects every non-`ResolveInput` action. Read the prompted
    /// investigator via [`GameState::current_mulligan`].
    Mulligan {
        /// Active investigators yet to mulligan, in player order; front =
        /// currently prompted.
        remaining: Vec<InvestigatorId>,
    },
    /// The Mythos step-1.4 encounter-draw loop (Rules Reference p.24), migrated
    /// off the former `GameState::mythos_draw_pending` cursor field (#348).
    /// `remaining[0]` is the investigator currently prompted to draw; the queue
    /// is the Active investigators in [`turn_order`](GameState::turn_order).
    /// Pushed by `mythos_phase`, advanced by `resume_encounter_draw` as each
    /// investigator confirms (running their full surge chain), popped when
    /// drained — at which point the post-1.4 `MythosAfterDraws` window opens.
    /// A mid-chain spawn-engagement tie pushes a
    /// [`SpawnEngage`](Continuation::SpawnEngage) frame *above* this one. While
    /// present, the engine rejects every non-`ResolveInput` action. Read the
    /// prompted drawer via [`GameState::current_encounter_drawer`].
    EncounterDraw {
        /// Active investigators yet to draw, in player order; front =
        /// currently prompted.
        remaining: Vec<InvestigatorId>,
    },
    /// A drawn encounter **treachery** whose Revelation is mid-resolution
    /// (#380). Pushed by `resolve_encounter_card` *before* it runs the
    /// Revelation; sits beneath any suspension the Revelation opens (a skill
    /// test, a choice, a nested effect). When that sub-resolution completes and
    /// this frame is top again, the **framework** disposes of the card
    /// (one-shot → `encounter_discard`; persistent → it placed itself during
    /// its Revelation, so skip) and pops. Suspension-reason-agnostic — replaces
    /// the former `pending_revelation_discard` slot, which only the skill-test
    /// driver flushed (so a choice-only Revelation was never disposed of).
    /// Never emits `AwaitingInput`.
    EncounterCard {
        /// The drawn treachery's card code, disposed of at teardown.
        card: CardCode,
    },
    /// The Mythos phase anchor (slice 1a, #393). Pushed at Mythos entry; sits
    /// beneath the phase's framework windows. On a child window's close the
    /// framework routes to the anchor's `on_child_pop` (keyed by `resume`).
    /// Never awaits input; popped when the phase transitions away.
    MythosPhase {
        /// Which child-pop boundary the anchor resumes at.
        resume: MythosResume,
    },
    /// The Investigation phase anchor (slice 1a, #393). See
    /// [`Continuation::MythosPhase`].
    InvestigationPhase {
        /// Which child-pop boundary the anchor resumes at.
        resume: InvestigationResume,
    },
    /// The Enemy phase anchor (slice 1a, #393). See [`Continuation::MythosPhase`].
    EnemyPhase {
        /// Which child-pop boundary the anchor resumes at.
        resume: EnemyResume,
        /// The investigator whose engaged enemies are currently attacking
        /// (Enemy step 3.3), or `None` before kickoff (the anchor is pushed
        /// ahead of hunter movement) and after the last investigator. The
        /// per-investigator cursor, lifted off the former
        /// `GameState::enemy_attack_pending` (#411, step 3 of #393).
        attacking: Option<InvestigatorId>,
    },
    /// The Upkeep phase anchor (slice 1a, #393). See [`Continuation::MythosPhase`].
    UpkeepPhase {
        /// Which child-pop boundary the anchor resumes at.
        resume: UpkeepResume,
    },
    /// The active investigator's open turn — Rules Reference step 2.2.1
    /// (slice 2a-i, #393). Pushed *above* the [`Continuation::InvestigationPhase`]
    /// anchor once the `InvestigatorTurnBegins` window closes; the anchor spans the
    /// whole phase beneath it. The player takes basic actions (each a typed
    /// `PlayerAction` today; a sub-resolution frame above this one tomorrow) while
    /// it is on top; `EndTurn` pops it via
    /// [`resume_end_turn`](crate::engine). Does **not** await `ResolveInput` — like
    /// the `TurnBegins` anchor it replaced, typed actions run against it (the idle
    /// outcome stays `Done`; surfacing the legal-action enumeration as
    /// `AwaitingInput` is slice 2b/#205).
    InvestigatorTurn {
        /// Whose turn this is. Mirrors [`GameState::active_investigator`] while on
        /// top; the durable source for the end-of-turn rotation.
        investigator: InvestigatorId,
        /// `true` once `end_turn`'s `EndOfTurn` forced effect suspended into a
        /// skill test before rotation (a single Frozen in Fear 01164), stranding
        /// the turn (slice 2a-i, #393 — absorbs the former
        /// `GameState::pending_end_turn`). The skill-test commit resume reads this
        /// to decide the resolved test triggers rotation; an ordinary mid-turn
        /// test leaves it `false`.
        ending: bool,
    },
    /// A parked enemy-attack loop, suspended because an attack opened a reaction
    /// window — either the soak window (`AfterEnemyAttackDamagedAsset`, after
    /// damage; C5b #237) or the before-attack cancel window (`BeforeEnemyAttack`,
    /// before damage; Axis D #336), distinguished by its `stage`. Pushed
    /// *beneath* that reaction window by the attack-loop driver
    /// (`drive_attack_loop` / `park_on_soak_window`); resumed by
    /// `resume_enemy_attack` (which pops it) once the window
    /// closes. An internal sequencing frame — never awaits player input itself
    /// (the window above it does); it is only ever momentarily on top inside
    /// `resume_enemy_attack`, between the window pop and its own pop. Lifted off
    /// the former `GameState::pending_enemy_attack` (#411, step 3 of #393).
    AttackLoop {
        /// The investigator whose engaged enemies are attacking.
        investigator: InvestigatorId,
        /// Attackers not yet resolved, in resolution order. The current attacker
        /// is still at the head for [`AttackLoopStage::BeforeAttack`] (it has not
        /// dealt yet); already removed for [`AttackLoopStage::AfterSoak`].
        remaining_attackers: Vec<EnemyId>,
        /// Which loop to re-enter.
        source: EnemyAttackSource,
        /// Where in the per-attacker sequence the loop suspended (Axis D #336).
        stage: AttackLoopStage,
    },
    /// An action paused over its attack-of-opportunity loop (#293, keystone of
    /// #393). Pushed above [`Self::InvestigatorTurn`] when an AoO-provoking action is
    /// taken; the `AoO` [`Self::AttackLoop`] is its child. On the loop's pop the
    /// `drive` loop resumes this frame: it re-validates (actor still active +
    /// the primary's precondition) and runs the primary effect, then pops.
    /// Transient — it persists across an `apply()` boundary only while a window
    /// suspends the loop. Never awaits input itself.
    ActionResolution {
        /// The acting investigator.
        investigator: InvestigatorId,
        /// Which primary effect to run when the `AoO` loop completes.
        resume: ActionResume,
    },
    /// An in-progress player distribution of an attack's / effect's damage +
    /// horror across eligible soakers and the investigator (#44/K5b, RR p.7).
    /// Accumulates `assignment` via per-point `PickSingle` prompts; when both
    /// `remaining_*` reach 0, the assignment is placed once (simultaneous) and
    /// the loop resumes by `source`. The top frame while prompting (it *is* the
    /// prompt); resumed via `ResolveInput` by `resume_damage_assignment`.
    DamageAssignment {
        /// The investigator taking the harm.
        investigator: InvestigatorId,
        /// Damage points still to assign.
        remaining_damage: u8,
        /// Horror points still to assign.
        remaining_horror: u8,
        /// Accumulating assignment (placed when both counters hit 0).
        assignment: Assignment,
        /// How to resume after placement.
        source: DamageSource,
    },
    /// A node of an in-progress card-effect walk (#422). The effect evaluator is
    /// frame-driven: each control-flow node parks here while its children
    /// resolve; the global `drive` loop steps the top frame. Replaces the former
    /// single-pass replay (`DecisionCursor`). A node that needs a controller pick
    /// suspends *in place* (its `Leaf` step returns `AwaitingInput` and the frame
    /// stays on top — it *is* the prompt), so this variant can await input
    /// (routed in `resolve_input`, like [`Self::DamageAssignment`]). Carries its
    /// own [`EvalContext`](crate::engine::EvalContext) snapshot (#345's grouped
    /// bindings) so resume re-binds without replay.
    Effect(EffectFrame),
}

/// One node of a frame-driven card-effect walk (#422). See
/// [`Continuation::Effect`]. Stepped by the evaluator's `step_effect_frame`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectFrame {
    /// A `Seq([..])` in progress: run `effects[next]`, advance `next` on each
    /// child pop, complete when `next == effects.len()`.
    Seq {
        /// The sequence's effects.
        effects: Vec<card_dsl::dsl::Effect>,
        /// Index of the next child to run.
        next: usize,
        /// The evaluation context for this sequence.
        ctx: crate::engine::EvalContext,
    },
    /// A `ForEachPointFailed(body)` in progress: run `body` `remaining` more
    /// times, decrementing on each child pop. Holds its own count on the stack
    /// so each iteration may suspend independently (fixes Grasping Hands).
    ForEachPointFailed {
        /// Iterations still to run.
        remaining: u8,
        /// The per-iteration body effect.
        body: Box<card_dsl::dsl::Effect>,
        /// The evaluation context for this loop.
        ctx: crate::engine::EvalContext,
    },
    /// A single effect node to evaluate. A terminal effect runs and pops;
    /// `ChooseOne` pushes its chosen branch; `Effect::Deal` may push a
    /// `DamageAssignment` (K5b-2); `Effect::Native { tag }` runs the native fn.
    /// **Suspends in place** for a controller pick (`ChooseOne`, a `*::Chosen`
    /// target, a native choice): the step returns `AwaitingInput` and the frame
    /// stays on top — it *is* the prompt. Resume re-steps it with
    /// `ctx.chosen_option` set; the node grounds/picks (checked indexing,
    /// validate-first) instead of suspending.
    Leaf {
        /// The effect node to evaluate.
        effect: Box<card_dsl::dsl::Effect>,
        /// The evaluation context for this node.
        ctx: crate::engine::EvalContext,
    },
}

/// Which action's primary effect a parked [`Continuation::ActionResolution`]
/// frame runs once its attack-of-opportunity loop completes (#293). The
/// basic-action variants carry only the action's *parameters*; board-dependent
/// values (Investigate difficulty, enemy presence) are re-derived live on
/// resume so a mid-action board change is reflected. The exception is
/// [`ActivateAbility`](ActionResume::ActivateAbility), which snapshots its
/// resolved effect (fixed at activation, not board-dependent) — see its docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResume {
    /// Relocate the investigator (and engaged enemies) to `destination`.
    Move { destination: LocationId },
    /// Begin the Investigate skill test on the investigator's location.
    Investigate,
    /// Gain 1 resource.
    Resource,
    /// Engage `enemy`.
    Engage { enemy: EnemyId },
    /// Draw 1 card (with the empty-deck penalty path).
    Draw,
    /// Run the activated ability's `effect` for `instance_id`'s source (#361).
    /// Unlike the basic actions, this snapshots the resolved `effect` rather
    /// than re-deriving it: an ability's effect is fixed at activation (not
    /// board-dependent), and the source may have self-discarded as a cost
    /// (First Aid 01019 depleting its last supply), so a live re-resolution by
    /// instance would be fragile. `instance_id` is kept only as the eval
    /// context's source.
    ActivateAbility {
        /// The source card instance — the eval context's `source` on resume.
        instance_id: CardInstanceId,
        /// The ability's effect, resolved at activation, run after the `AoO` loop.
        effect: card_dsl::dsl::Effect,
    },
    /// Complete a non-fast card play after its `AoO` loop (#378): run the card's
    /// `OnPlay` effects and, for an asset, move it into play. The card has
    /// already been announced (`CardPlayed`; an event has also left hand and is
    /// stashed for discard-on-completion). `hand_index` locates an asset still
    /// in hand (unused for an already-stashed event); `code` re-derives the
    /// destination + `OnPlay` abilities from the registry on resume.
    PlayCard {
        /// The asset's hand slot (still in hand until its `OnPlay` resolves).
        hand_index: u8,
        /// The played card's code — re-derives destination + abilities on resume.
        code: CardCode,
    },
}

impl Continuation {
    /// True if this is a `*Phase` anchor (slice 1b, #393): an inert framework
    /// frame the main loop's `drive` advances, never one that awaits player
    /// input itself. Everything else on top of the stack *is* awaiting input.
    #[must_use]
    pub fn is_phase_anchor(&self) -> bool {
        matches!(
            self,
            Continuation::MythosPhase { .. }
                | Continuation::InvestigationPhase { .. }
                | Continuation::EnemyPhase { .. }
                | Continuation::UpkeepPhase { .. }
        )
    }

    /// True if this top frame is a mandatory prompt that only `ResolveInput` may
    /// advance (slice 1b, #393): a reaction/forced window, skill-test commit,
    /// choice, hunter/spawn pick, hand-size discard, act round-end, substitution
    /// prompt, mulligan, or encounter draw.
    ///
    /// Two exceptions return `false` — the engine accepts other actions:
    /// - **`*Phase` anchors** are inert / the open turn, so typed actions run.
    /// - a **Fast-play window** — a [`FastWindow`](Continuation::FastWindow) with
    ///   *no* pending candidates — is a play *opportunity*, not a mandatory prompt:
    ///   Fast `PlayCard`/`ActivateAbility` are allowed (the handlers gate
    ///   eligibility) and `ResolveInput::Skip` closes it. A window *with* pending
    ///   triggers (reaction or forced) does await `ResolveInput`.
    ///
    /// (`EncounterCard` is framework-internal and never sits on top at an action
    /// boundary, so its `true` here is moot.)
    #[must_use]
    pub fn awaits_input(&self) -> bool {
        match self {
            // A window/run awaits a mandatory `ResolveInput` iff it has
            // candidates to resolve. An empty framework Fast-gate window
            // (`FastWindow` with no pending plays) is *permissive* — the player
            // may act but is not required to, so it does not block other actions.
            Continuation::TimingPointWindow { .. } | Continuation::FastWindow { .. } => {
                self.pending_candidates().is_some_and(|c| !c.is_empty())
            }
            // Neither is a mandatory ResolveInput prompt. The open turn takes
            // typed actions (Move/Investigate/Fight/…), not ResolveInput (slice
            // 2a-i, #393). The parked attack loop is internal sequencing: the
            // reaction window pushed above it is the player-facing prompt, not
            // this frame — it is only ever momentarily on top inside
            // `resume_enemy_attack`, never at a suspension boundary (#411).
            Continuation::InvestigatorTurn { .. }
            | Continuation::AttackLoop { .. }
            | Continuation::ActionResolution { .. } => false,
            other => !other.is_phase_anchor(),
        }
    }

    /// The resolution candidates of an open window/run on the stack —
    /// a [`TimingPointWindow`](Self::TimingPointWindow) (event windows + the
    /// #213 forced run) or a [`FastWindow`](Self::FastWindow) (framework
    /// windows). Lets the shared resolution driver read candidates without
    /// caring which window frame it is. `None` for any other frame.
    #[must_use]
    pub fn pending_candidates(&self) -> Option<&Vec<ResolutionCandidate>> {
        match self {
            Continuation::TimingPointWindow { candidates, .. }
            | Continuation::FastWindow { candidates, .. } => Some(candidates),
            _ => None,
        }
    }

    /// Mutable counterpart to [`Self::pending_candidates`].
    pub fn pending_candidates_mut(&mut self) -> Option<&mut Vec<ResolutionCandidate>> {
        match self {
            Continuation::TimingPointWindow { candidates, .. }
            | Continuation::FastWindow { candidates, .. } => Some(candidates),
            _ => None,
        }
    }

    /// Whether the frame is the mandatory #213 forced run, in either
    /// representation. `false` for reaction windows and non-window frames.
    #[must_use]
    pub fn is_forced(&self) -> bool {
        match self {
            Continuation::TimingPointWindow { mode, .. } => matches!(mode, TimingMode::Forced(_)),
            _ => false,
        }
    }

    /// The [`ForcedContinuation`] if this frame is the forced run, in either
    /// representation; `None` for a reaction window or non-window frame.
    #[must_use]
    pub fn forced_continuation(&self) -> Option<ForcedContinuation> {
        match self {
            Continuation::TimingPointWindow {
                mode: TimingMode::Forced(c),
                ..
            } => Some(*c),
            _ => None,
        }
    }

    /// The [`TimingEvent`](crate::engine::TimingEvent) that opened this frame,
    /// if it is a [`TimingPointWindow`](Self::TimingPointWindow) (event window
    /// or forced run). `None` for [`FastWindow`](Self::FastWindow) framework
    /// windows (no timing event) and non-window frames. Lets the driver bind
    /// event-specific `EvalContext` (the attacking enemy, the would-be discovery
    /// count) directly from the timing event (#433).
    #[must_use]
    pub fn window_timing_event(&self) -> Option<&crate::engine::TimingEvent> {
        match self {
            Continuation::TimingPointWindow { event, .. } => Some(event),
            _ => None,
        }
    }

    /// Whether `investigator` may submit a Fast action into this open window. A
    /// [`FastWindow`](Self::FastWindow) delegates to its [`FastActorScope`]; a
    /// [`TimingPointWindow`](Self::TimingPointWindow) reaction window admits any
    /// investigator (reaction windows carried `FastActorScope::Any`). Forced
    /// runs and non-window frames admit none.
    #[must_use]
    pub fn permits_fast(&self, investigator: InvestigatorId) -> bool {
        match self {
            Continuation::FastWindow { fast_actors, .. } => fast_actors.permits(investigator),
            Continuation::TimingPointWindow {
                mode: TimingMode::Reaction,
                ..
            } => true,
            _ => false,
        }
    }
}

/// The Mythos-phase child-pop boundary an anchor resumes at (slice 1a, #393).
/// Names the framework window whose close re-enters the Mythos driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MythosResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening
    /// (round bump, `PhaseStarted`, steps 1.1–1.4) and replaces this with the
    /// running anchor.
    Entry,
    /// Post-step-1.4 (encounter draws done) window closed; run `mythos_phase_end`.
    AfterDraws,
}

/// The Investigation-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvestigationResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Post-2.1 window closed; begin the first investigator's turn.
    Begins,
    /// Post-2.2 turn-begins window closed; the investigator now acts (no
    /// continuation work — slice 2 makes this an `InvestigatorTurn` frame).
    TurnBegins,
}

/// The Enemy-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Before-investigator-attacked window closed; resolve this investigator's
    /// attacks (step 3.3).
    BeforeInvestigatorAttacked,
    /// After-all-investigators-attacked window closed; run `enemy_phase_end`.
    AfterAllAttacked,
}

/// The Upkeep-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpkeepResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Post-4.1 window closed; run `upkeep_resume` (steps 4.2–4.6).
    Begins,
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
    /// `advance`. Initialized to
    /// [`SkillTestStep::AwaitingCommit`] at
    /// `start_skill_test`; advanced in lock-step as the resolution
    /// sequence runs. Post-commit variants carry the test's outcome
    /// as a `succeeded` payload (see [`SkillTestStep`]) so the
    /// invariant "outcome is known iff the test is past the commit
    /// window" is structural.
    pub continuation: SkillTestStep,
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
/// call to `advance`.
///
/// The driver (`advance`, with the resolution body in `run_resolution`)
/// walks a fixed sequence of steps:
///
/// 1. Validate commits + draw chaos token + emit
///    [`SkillTestSucceeded`](crate::Event::SkillTestSucceeded) /
///    [`SkillTestFailed`](crate::Event::SkillTestFailed)
/// 2. Apply the action-specific
///    [`SkillTestFollowUp`] (Investigate / Fight / Evade / None) —
///    this is where `damage_enemy` may emit
///    [`EnemyDefeated`](crate::Event::EnemyDefeated) and queue an
///    an after-enemy-defeated reaction window
/// 3. Fire
///    [`OnSkillTestResolution`](crate::dsl::Trigger::OnSkillTestResolution)
///    triggers on committed cards
/// 4. Discard committed cards + emit
///    [`SkillTestEnded`](crate::Event::SkillTestEnded) + drain
///    pending modifiers
///
/// After each step that *can* queue a reaction window, the driver checks
/// whether that window is now the top frame; if so it suspends with
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput) and yields to the
/// `drive` loop, which dispatches the window. On the window's close the loop
/// re-dispatches this `SkillTest` frame, which reads its cursor and jumps to
/// the matching step (Slice C-plumbing). This is the rules-correct shape per
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
/// - [`PreCommitWindow`](Self::PreCommitWindow) — initial state; `advance` opens
///   the ST.1→ST.2 player window, then pre-advances to `AwaitingCommit`.
/// - [`AwaitingCommit`](Self::AwaitingCommit) — `advance`'s `AwaitingCommit` arm
///   emits the commit-window
///   [`ResolveInput`](crate::action::PlayerAction::ResolveInput)
///   prompt with a [`PickMultiple`](crate::action::InputResponse::PickMultiple)
///   response (each `OptionId` a hand index).
/// - [`PreTokenWindow`](Self::PreTokenWindow) — set after the commit; `advance`
///   opens the ST.2→ST.3 player window, then pre-advances to `Resolving`.
/// - [`Resolving`](Self::Resolving) — set by `finish_skill_test` once the
///   commit is validated and stored. The next driver iteration runs steps 1–2
///   (`run_resolution`) and pre-advances to [`PostFollowUp`](Self::PostFollowUp).
/// - [`PostFollowUp`](Self::PostFollowUp) — set once steps 1–2 have run.
///   The next driver iteration runs step 3.
/// - [`PostOnResolution`](Self::PostOnResolution) — set after step 3.
///   The next driver iteration runs step 4 (terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SkillTestStep {
    /// The RR p.26 player window after ST.1 (skill determined) and before ST.2
    /// (commit). The initial state at skill-test start. `advance` opens the
    /// window here, pre-advancing to [`AwaitingCommit`](Self::AwaitingCommit).
    /// (#374.)
    PreCommitWindow,
    /// Initial state: waiting on the commit-window
    /// [`ResolveInput`](crate::action::PlayerAction::ResolveInput).
    AwaitingCommit,
    /// The RR p.26 player window after ST.2 (commit) and before ST.3 (reveal
    /// chaos token). Set by `finish_skill_test` once the commit is stored;
    /// `advance` opens the window here, pre-advancing to
    /// [`Resolving`](Self::Resolving). (#374.)
    PreTokenWindow,
    /// Commit submitted: the next driver iteration runs the resolution
    /// body (sum committed icons, fire `OnCommit`, resolve the chaos
    /// token, run the action follow-up + `on_success` / `on_fail`), then
    /// pre-advances to [`PostFollowUp`](Self::PostFollowUp).
    Resolving,
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
/// actions while a window frame is the top of the window stack.
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

/// The framework step a [`FastWindow`](Continuation::FastWindow) gates — the
/// discriminant for the engine's framework player windows. Routes the close
/// continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastWindowKind {
    /// A Rules-Reference timing-step player window. Close routes to the
    /// `*Phase` anchor beneath via `anchor_on_child_pop`; the [`PhaseStep`]
    /// names the timing point (the anchor's `resume` is the real continuation
    /// key, slice 1a #393).
    Phase(PhaseStep),
    /// A skill-test player window (#374). Close re-enters the skill-test
    /// driver.
    SkillTest {
        /// ST.1 (pre-commit) vs ST.2 (pre-token) — distinguishes the two
        /// skill-test windows for routing.
        before_token: bool,
    },
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

/// How a forced run ([`TimingMode::Forced`]) resumes the framework flow
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

/// Whether a [`TimingPointWindow`](Continuation::TimingPointWindow) is a
/// skippable reaction window or the mandatory #213 forced run. Collapses the
/// old `ResolutionKind::{Window | Forced}` split onto the one frame: a forced
/// run admits no Fast plays, drains all candidates, and on close resumes the
/// framework flow named by its [`ForcedContinuation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingMode {
    /// A reaction/fast window: skippable, admits Fast plays.
    Reaction,
    /// The forced run (#213): mandatory, no Fast plays; resumes the framework
    /// flow named by the continuation on close.
    Forced(ForcedContinuation),
}

/// The Rules-Reference timing step a [`FastWindowKind::Phase`] window sits
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
    /// on the [`EnemyPhase`](Continuation::EnemyPhase) anchor's `attacking`
    /// cursor (#411), not in the variant — mirror of [`MythosAfterDraws`] (the
    /// encounter-draw loop's analog lives on the
    /// [`EncounterDraw`](Continuation::EncounterDraw) frame).
    ///
    /// Continuation (in `anchor_on_child_pop`): read the cursor,
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
/// movement is a `PickSingle` over a prey-filtered destination set
/// (the chosen prey doesn't persist, so picking a location is
/// outcome-equivalent to picking an investigator-then-path); engagement
/// on arrival is a `PickSingle` over the co-located set.
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
/// `PickSingle`. `investigator_to_draw` is the drawing
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
/// window frame.
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
    /// construction) can build a window's pending candidates directly.
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
    /// The skill test currently in flight, if any; `None` outside a test. Reads
    /// the topmost `Continuation::SkillTest` frame — the continuation stack is
    /// the single source of truth for "a test is mid-resolution" (#348). Topmost
    /// (not `.last()`) because a reaction window can sit above the test mid-
    /// resolution; "topmost `SkillTest` = the in-flight test".
    #[must_use]
    pub fn current_skill_test(&self) -> Option<&InFlightSkillTest> {
        self.continuations.iter().rev().find_map(|c| match c {
            Continuation::SkillTest(t) => Some(t),
            _ => None,
        })
    }

    /// Mutable counterpart to [`Self::current_skill_test`].
    pub fn current_skill_test_mut(&mut self) -> Option<&mut InFlightSkillTest> {
        self.continuations.iter_mut().rev().find_map(|c| match c {
            Continuation::SkillTest(t) => Some(t),
            _ => None,
        })
    }

    /// Remove and return the in-flight skill test (popping its frame off the
    /// continuation stack). Called at test teardown.
    pub fn take_skill_test(&mut self) -> Option<InFlightSkillTest> {
        let pos = self
            .continuations
            .iter()
            .rposition(|c| matches!(c, Continuation::SkillTest(_)))?;
        match self.continuations.remove(pos) {
            Continuation::SkillTest(t) => Some(t),
            _ => unreachable!("rposition matched SkillTest"),
        }
    }

    /// The investigator currently prompted to mulligan, if a setup mulligan is
    /// in progress; `None` otherwise. Reads the top
    /// [`Continuation::Mulligan`] frame's `remaining[0]` — the continuation
    /// stack is the single source of truth for "a mulligan is pending" (#348,
    /// replacing the former `mulligan_pending` cursor). The frame is only ever
    /// the top during setup, so `.last()` (not a topmost search) is correct.
    #[must_use]
    pub fn current_mulligan(&self) -> Option<InvestigatorId> {
        match self.continuations.last() {
            Some(Continuation::Mulligan { remaining }) => remaining.first().copied(),
            _ => None,
        }
    }

    /// The investigator currently prompted to draw their Mythos step-1.4
    /// encounter card, if an encounter-draw loop is in progress; `None`
    /// otherwise. Reads the topmost [`Continuation::EncounterDraw`] frame's
    /// `remaining[0]` — the continuation stack is the single source of truth
    /// for "an encounter draw is pending" (#348, replacing the former
    /// `mythos_draw_pending` cursor). Topmost (not `.last()`) because a
    /// mid-chain [`SpawnEngage`](Continuation::SpawnEngage) frame can sit above
    /// the draw frame while a spawn-engagement tie is resolved.
    #[must_use]
    pub fn current_encounter_drawer(&self) -> Option<InvestigatorId> {
        self.continuations.iter().rev().find_map(|c| match c {
            Continuation::EncounterDraw { remaining } => remaining.first().copied(),
            _ => None,
        })
    }

    /// Whether a skill test is currently in flight.
    #[must_use]
    pub fn has_skill_test_in_flight(&self) -> bool {
        self.continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_)))
    }

    /// Iterator over the open windows on the continuation stack, in stack
    /// order (bottom to top). The windows are `TimingPointWindow` / `FastWindow`
    /// frames; non-window frames are skipped.
    /// Every open window/run frame on the stack, in stack order — legacy
    /// [`FastWindow`](Continuation::FastWindow) framework windows **and**
    /// [`TimingPointWindow`](Continuation::TimingPointWindow) event windows /
    /// forced runs (#433). A frame is a window/run iff it carries a candidate
    /// list ([`Continuation::pending_candidates`]).
    fn windows(&self) -> impl DoubleEndedIterator<Item = &Continuation> {
        self.continuations
            .iter()
            .filter(|c| c.pending_candidates().is_some())
    }

    /// The open windows as a `Vec` of references, in stack order. Read
    /// accessor for callers (and tests) that inspect the window stack the
    /// way they used to read the former `open_windows` field.
    #[must_use]
    pub fn open_windows(&self) -> Vec<&Continuation> {
        self.windows().collect()
    }

    /// The topmost open window regardless of pending triggers (the former
    /// `open_windows.last()`), e.g. for the Fast-play `permissive_window`
    /// timing gate — including a pure-Fast gate with empty `pending_triggers`.
    #[must_use]
    pub fn top_window(&self) -> Option<&Continuation> {
        self.windows().next_back()
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
        // A framework window is a `FastWindow` frame on the stack (#433); the
        // whole `Continuation` serializes for replay.
        let window = Continuation::FastWindow {
            candidates: Vec::new(),
            fast_actors: FastActorScope::Any,
            kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        };
        let json = serde_json::to_string(&window).expect("serialize");
        let back: Continuation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, window);
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
    fn awaits_input_gates_suspensions_but_not_anchors_or_fast_windows() {
        // slice 1b: the one guard rule keys off this. Phase anchors are inert
        // (open turn / loop-driven), so typed actions run there.
        assert!(!Continuation::InvestigationPhase {
            resume: InvestigationResume::TurnBegins,
        }
        .awaits_input());
        assert!(!Continuation::MythosPhase {
            resume: MythosResume::Entry,
        }
        .awaits_input());
        // A Fast-play window (a `FastWindow` with no pending candidates) is a
        // play opportunity, not a mandatory prompt — Fast plays stay allowed.
        assert!(!Continuation::FastWindow {
            candidates: Vec::new(),
            fast_actors: FastActorScope::Any,
            kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
        }
        .awaits_input());
        // Every other suspension hits the `_ => true` arm and awaits
        // ResolveInput. This includes a `Choice` (e.g. a `ChooseOne` OnPlay
        // event mid-resolution) and a `SubstitutionPrompt`, which the former
        // eight-block guard ladder did NOT explicitly gate — the unified rule
        // now correctly rejects typed actions while one is on top.
        assert!(Continuation::SubstitutionPrompt {
            investigator: InvestigatorId(1),
        }
        .awaits_input());
        assert!(Continuation::Mulligan { remaining: vec![] }.awaits_input());
        assert!(Continuation::EncounterDraw { remaining: vec![] }.awaits_input());
    }

    #[test]
    fn investigator_turn_frame_classification() {
        let frame = Continuation::InvestigatorTurn {
            investigator: InvestigatorId(1),
            ending: false,
        };
        // The open turn is not a framework anchor...
        assert!(!frame.is_phase_anchor());
        // ...and it does NOT await ResolveInput — typed actions (Move, Fight, …)
        // run against it, exactly as they ran against the TurnBegins anchor.
        assert!(!frame.awaits_input());
        // It carries no window candidates.
        assert!(frame.pending_candidates().is_none());
    }

    #[test]
    fn investigator_turn_frame_round_trips_both_ending_states() {
        // The frame is replay state (the `ending` flag absorbed the former
        // `pending_end_turn`), so both flag values must serialize round-trip.
        for ending in [false, true] {
            let frame = Continuation::InvestigatorTurn {
                investigator: InvestigatorId(1),
                ending,
            };
            let json = serde_json::to_string(&frame).unwrap();
            let back: Continuation = serde_json::from_str(&json).unwrap();
            assert_eq!(frame, back);
        }
    }

    #[test]
    fn phase_anchor_variants_round_trip_and_are_not_resolution_windows() {
        let anchors = [
            Continuation::MythosPhase {
                resume: MythosResume::AfterDraws,
            },
            Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            },
            Continuation::EnemyPhase {
                resume: EnemyResume::BeforeInvestigatorAttacked,
                attacking: Some(InvestigatorId(3)),
            },
            Continuation::UpkeepPhase {
                resume: UpkeepResume::Begins,
            },
        ];
        for a in anchors {
            // Anchors are framework frames, never reaction windows.
            assert!(a.pending_candidates().is_none());
            // Serializable like every other frame.
            let json = serde_json::to_string(&a).unwrap();
            let back: Continuation = serde_json::from_str(&json).unwrap();
            assert_eq!(a, back);
        }
    }

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
    fn open_window_lives_on_the_continuation_stack_as_a_fast_window() {
        // A framework window is a `Continuation::FastWindow` frame on the one
        // stack (#433 A-ii) — there is no separate `open_windows` Vec.
        let state = GameStateBuilder::new()
            .with_open_window(
                FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                FastActorScope::Any,
            )
            .build();
        assert_eq!(state.continuations.len(), 1);
        assert!(matches!(
            state.continuations[0],
            Continuation::FastWindow { .. }
        ));
        // The read accessor surfaces it as the former `open_windows` view.
        assert_eq!(state.open_windows().len(), 1);
        assert!(matches!(
            state.open_windows()[0],
            Continuation::FastWindow {
                kind: FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
                ..
            }
        ));
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
mod encounter_draw_tests {
    use crate::test_support::GameStateBuilder;

    #[test]
    fn game_state_default_has_no_encounter_draw_pending() {
        let state = GameStateBuilder::new().build();
        assert_eq!(state.current_encounter_drawer(), None);
    }
}

#[cfg(test)]
mod enemy_attack_loop_tests {
    use super::*;
    use crate::state::InvestigatorId;
    use crate::test_support::GameStateBuilder;

    #[test]
    fn enemy_phase_anchor_attacking_round_trips_through_serde() {
        use crate::state::{Continuation, EnemyResume};
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(InvestigatorId(7)),
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
    }

    #[test]
    fn damage_assignment_frame_round_trips_through_serde() {
        use crate::state::{Assignment, Continuation, DamageSource, EnemyAttackSource, EnemyId};
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::DamageAssignment {
            investigator: InvestigatorId(1),
            remaining_damage: 2,
            remaining_horror: 0,
            assignment: Assignment::default(),
            source: DamageSource::EnemyAttack {
                enemy: EnemyId(5),
                remaining_attackers: vec![EnemyId(6)],
                attack_source: EnemyAttackSource::EnemyPhase,
            },
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
    }

    #[test]
    fn attack_loop_frame_round_trips_through_serde() {
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, EnemyId};
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::AttackLoop {
            investigator: InvestigatorId(7),
            remaining_attackers: vec![EnemyId(2), EnemyId(3)],
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::AfterSoak,
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
    }

    #[test]
    fn attack_loop_pick_order_stage_round_trips_through_serde() {
        use crate::state::{AttackLoopStage, Continuation, EnemyAttackSource, EnemyId};
        let mut state = GameStateBuilder::new().build();
        state.continuations.push(Continuation::AttackLoop {
            investigator: InvestigatorId(1),
            remaining_attackers: vec![EnemyId(2), EnemyId(3)],
            source: EnemyAttackSource::EnemyPhase,
            stage: AttackLoopStage::PickOrder,
        });
        let json = serde_json::to_string(&state).expect("serialize");
        let back: GameState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.continuations, state.continuations);
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

#[cfg(test)]
mod action_resolution_frame_tests {
    use super::*;

    #[test]
    fn action_resolution_frame_never_awaits_input_and_is_not_a_phase_anchor() {
        let f = Continuation::ActionResolution {
            investigator: InvestigatorId(1),
            resume: ActionResume::Resource,
        };
        assert!(
            !f.awaits_input(),
            "a mid-action frame is internal, never a prompt"
        );
        assert!(
            !f.is_phase_anchor(),
            "a mid-action frame is not a phase anchor"
        );
    }
}

#[cfg(test)]
mod effect_frame_tests {
    use crate::dsl::Effect;
    use crate::engine::EvalContext;
    use crate::state::{Continuation, EffectFrame, InvestigatorId};

    #[test]
    fn effect_frame_variant_roundtrips_serde() {
        let frame = Continuation::Effect(EffectFrame::Seq {
            effects: vec![Effect::Seq(vec![])],
            next: 0,
            ctx: EvalContext::for_controller(InvestigatorId(1)),
        });
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: Continuation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }
}
