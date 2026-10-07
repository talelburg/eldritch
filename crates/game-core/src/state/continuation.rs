//! The continuation stack's frames: the [`Continuation`] enum, the payload
//! and step types that exist only to serve a frame, and the per-frame
//! classifiers.

use std::collections::{BTreeMap, BTreeSet};
use std::{iter, mem};

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{ActionDesignator, Effect, EventTiming, SkillTestKind};
use serde::{Deserialize, Serialize};

use crate::engine::evaluator::EvalContext;
use crate::engine::TimingEvent;
use crate::event::FailureReason;
use crate::state::continuation::frame::impl_frame;
use crate::state::game_state::{DifficultyBasis, SkillTestId};
use crate::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId,
    LocationId,
};

mod frame;
mod stack;

pub use frame::Frame;
pub use stack::ContinuationStack;

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

/// Where a parked enemy-attack loop stands in its per-attacker sequence (#704).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackLoopStage {
    /// The head attacker's `EnemyAttacks` triggering condition is walking its
    /// `when → resolve → at → after` sequence on the coordinator frame above
    /// this one (#704). The head is still at the front of `remaining_attackers`
    /// — the loop takes it off when the coordinator pops and this frame is
    /// re-exposed, exhausting it (enemy phase only) before moving on.
    ///
    /// Not a suspension: the `drive` loop dispatches this frame directly. It
    /// exists so the attack's sequence can suspend *anywhere* inside itself (a
    /// `when` cancel window, the soak distribution prompt, an `after` reaction)
    /// without the loop having to read the stack after emitting — the shape ADR
    /// 0003 forbids, and the reason the attack was the last condition to bypass
    /// the coordinator.
    Attacking,
    /// Suspended on the player's attack-order `PickSingle` (#143/K4): 2+
    /// attackers remain and none has dealt this iteration. The `AttackLoop`
    /// frame is the **top** frame and *is* the prompt. Resume reorders
    /// `remaining_attackers` to put the picked enemy at the head and begins its
    /// attack. Unlike [`Attacking`](Self::Attacking), which the `drive` loop
    /// dispatches, this stage resumes on `ResolveInput` via
    /// `resume_attack_order_pick`.
    PickOrder,
}

/// A computed damage/horror distribution for one source of harm (C5b #237).
///
/// How much of the harm's damage and horror lands on the defending investigator
/// versus each soak-bearing asset. Built by `assign_attack` (soak-first) or the
/// interactive per-point distribution (#44/K5b); placed simultaneously by
/// `place_assignment`, per Rules Reference page 7's "Apply Damage/Horror" clause.
/// Lives here (not in `engine`) because the live one is owned by the
/// [`Continuation::DealDamage`] frame that walks the two steps, and each of the
/// two triggering conditions snapshots it into its own event
/// (`docs/adr/0009-damage-is-assigned-then-placed.md`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Damage absorbed by the investigator.
    pub investigator_damage: u8,
    /// Horror absorbed by the investigator.
    pub investigator_horror: u8,
    /// instance → damage soaked onto that asset.
    pub asset_damage: BTreeMap<CardInstanceId, u8>,
    /// instance → horror soaked onto that asset.
    pub asset_horror: BTreeMap<CardInstanceId, u8>,
}

/// What is dealing the damage/horror a [`Continuation::DealDamage`] frame is
/// walking — carried on the frame and snapshotted into both of its triggering
/// conditions, and read at
/// [`DealDamageStep::Finish`] to decide how the frame
/// resumes its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageSource {
    /// An enemy attack. Guard Dog 01021's *"When an enemy attack deals damage to
    /// Guard Dog"* is scoped to this arm, and the reaction window binds `enemy`
    /// as the attacker its retaliate names. The frame simply pops at `Finish`:
    /// the rest of the attack — its `at`/`after` cells, and the exhaust the
    /// parked [`Continuation::AttackLoop`] owns — is on the frames beneath
    /// (#704).
    EnemyAttack {
        /// The attacking enemy (binds the retaliate's target).
        enemy: EnemyId,
    },
    /// A card/treachery `Effect::Deal` (K5b-2): at `Finish`, resume the parked
    /// effect walk so any remaining iterations run.
    Effect,
}

/// Where a [`Continuation::DealDamage`] frame stands in the Rules Reference's
/// two-step procedure for dealing damage/horror
/// (`glossary/Dealing_Damage_Horror.md`), plus the bookends that get it there
/// and hand back to the caller.
///
/// The two steps are two triggering conditions with a named window between
/// them, and ADR 0003 forbids emitting both synchronously from one call — so the
/// cursor is not organisation, it is the only legal way to sequence them. See
/// `docs/adr/0009-damage-is-assigned-then-placed.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DealDamageStep {
    /// Rules Reference step 1's determination: distribute the harm across
    /// eligible soakers and the investigator, one point at a time (#44/K5b, RR
    /// p.7). Drains immediately when no point is contested, so the cursor always
    /// starts at the top; while a point *is* contested this frame is the top
    /// frame and **is** the prompt, resumed by its `PickSingle`.
    Distribute {
        /// Damage points still to assign.
        remaining_damage: u8,
        /// Horror points still to assign.
        remaining_horror: u8,
    },
    /// Emit [`DamageAssigned`](crate::engine::TimingEvent::DamageAssigned) — a
    /// bare milestone, so its whole sequence is the three cells around a no-op
    /// resolve step. Advances the cursor before emitting (tail position).
    Announce,
    /// Emit [`DamagePlaced`](crate::engine::TimingEvent::DamagePlaced), whose
    /// resolve step is the simultaneous placement and the defeat sweep. Re-reads
    /// the frame's assignment, so it carries whatever `DamageAssigned`'s cells
    /// made of it. Advances the cursor before emitting (tail position).
    Place,
    /// Both conditions have run: pop and resume by
    /// [`DamageSource`].
    Finish,
}

/// Which deck an [`AdvanceReverse`](Continuation::AdvanceReverse) frame advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceDeck {
    /// The act deck (`act_index` / clue thresholds).
    Act,
    /// The agenda deck (`agenda_index` / doom thresholds).
    Agenda,
}

/// Step cursor for the [`AdvanceReverse`](Continuation::AdvanceReverse) frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceStep {
    /// Push the observable `…Advanced` event; for a **forced** advance in
    /// interactive mode, suspend here with a one-option on-card pick (the flip,
    /// anchored to the act/agenda — cursor stays until resumed). A **deliberate**
    /// advance skips the pause and falls through (#558).
    AwaitAck,
    /// Queue the leaving card's Forced on-advance reverse via `queue_event` —
    /// the frame lands above this one and the `drive` loop resolves it, which is
    /// what re-exposes this frame at [`Finalize`](Self::Finalize) (ADR 0003).
    FireReverse,
    /// The reverse has resolved: bump the deck cursor and pop the frame.
    Finalize,
}

/// Why an act/agenda is advancing — decides whether the acknowledge prompts.
///
/// A **forced** advance (agenda doom threshold; act 01110 on Ghoul Priest
/// defeat) surfaces the flip as an on-card pick the player clicks. A
/// **deliberate** advance (the `AdvanceAct` action, or the round-end
/// objective) was already the player's choice, so the ack is skipped — the
/// action *was* the flip (#558).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceTrigger {
    /// Game-forced (doom threshold / a forced ability). Prompts the flip pick.
    Forced,
    /// Player-chosen (spend clues / round-end objective). The advance action
    /// was the flip, so the ack is skipped.
    Deliberate,
}

/// **Why an asset instance is entering an investigator's play area** — which
/// decides whether the arrival is announced as *entering play* (#772).
///
/// The two paths share the slot make-room machinery, because
/// `glossary/Slots.md` writes them into one sentence — *"If playing **or
/// gaining control** of an asset would put an investigator above his or her slot
/// limit …"* — but only one of them is a card entering play. A card whose
/// control changes was already in play and does not enter it again, so
/// announcing it would fire every after-enters-play reaction a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetEntry {
    /// Played from hand (RR Appendix I step 4, *"placed in play"*). Announced
    /// via the `EnteredPlay` timing event.
    PlayedFromHand,
    /// Control of an already-in-play card was taken
    /// ([`Effect::TakeControl`]). Not
    /// announced: the card never left play, so it does not re-enter it.
    ControlTaken,
}

/// A frame on the [`GameState::continuations`](crate::state::game_state::GameState::continuations) suspend/resume stack
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
    /// See [`TimingPointWindowFrame`].
    TimingPointWindow(TimingPointWindowFrame),
    /// See [`FastWindowFrame`].
    FastWindow(FastWindowFrame),
    /// An act/agenda is advancing (#482). A small resumable sub-process that
    /// pushes the observable `…Advanced` event, for a forced advance in
    /// interactive mode pauses for a one-option on-card flip pick (#558), fires
    /// the leaving card's Forced on-advance reverse (which may itself suspend —
    /// 01105's interactive `ChooseOne`), then bumps the deck cursor *after* the
    /// reverse resolves (RR order). Driven by the `drive` loop and resumed via
    /// `resolve_input` (mirrors the `SkillTest` frame). Replaces the former
    /// synchronous `advance_agenda`/`advance_act` emit-then-bump, whose
    /// post-forced bookkeeping stranded a suspending reverse.
    AdvanceReverse {
        /// Which deck is advancing.
        deck: AdvanceDeck,
        /// Cursor index of the leaving card (before the bump).
        from: usize,
        /// Printed code of the leaving card (its reverse fires).
        leaving_code: CardCode,
        /// Where in the sub-process we are.
        step: AdvanceStep,
        /// Whether this advance was forced (prompts the flip pick) or
        /// deliberate (the ack is skipped) (#558).
        trigger: AdvanceTrigger,
    },
    /// See [`AcknowledgeForcedFrame`].
    AcknowledgeForced(AcknowledgeForcedFrame),
    /// A skill test is mid-resolution. Carries the in-flight test's data
    /// directly (the former `GameState::in_flight_skill_test` singleton, folded
    /// onto the frame — #348). Pushed at test start; popped when the test fully
    /// resolves (Axis-B T4). At most one is ever on the stack (no nesting today);
    /// [`GameState::current_skill_test`](crate::state::game_state::GameState::current_skill_test) returns the topmost one.
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
    /// See [`EmitEventFrame`].
    EmitEvent(EmitEventFrame),
    /// See [`TimingPointFrame`].
    TimingPoint(TimingPointFrame),
    /// See [`SubstitutionPromptFrame`].
    SubstitutionPrompt(SubstitutionPromptFrame),
    /// See [`MulliganFrame`].
    Mulligan(MulliganFrame),
    /// See [`EncounterDrawFrame`].
    EncounterDraw(EncounterDrawFrame),
    /// See [`PlayerDrawFrame`].
    PlayerDraw(PlayerDrawFrame),
    /// See [`EncounterCardFrame`].
    EncounterCard(EncounterCardFrame),
    /// See [`PlayFromHandFrame`].
    PlayFromHand(PlayFromHandFrame),
    /// The entered-location half of a Move, parked beneath the whole
    /// `LeftLocation` sequence (#569). Pushed by `move_primary_effect`
    /// immediately before it emits `LeftLocation` — and since #721 that emit
    /// carries the relocation too, which the coordinator performs at the
    /// condition's own resolve step *above* this frame, so Barricade 01038's
    /// self-discard resolves before the departure lands. When the loop
    /// re-exposes this frame it pops, reveals the destination, auto-engages its
    /// ready enemies, and emits `EnteredLocation` in tail position.
    ///
    /// Exists because the emit queues rather than resolves (ADR 0003): running
    /// the engage + entered-location emit inline after it pushed them *above*
    /// the left-location abilities, inverting the two. Framework-internal:
    /// [Driven](FrameActivity::Driven), only ever momentarily on top inside
    /// `drive`, so it never awaits input and `resolve_input` rejects it.
    /// Deliberately narrow rather than a resumable `ActionResolution`: no other
    /// primary needs one today (#612).
    MoveEnter {
        /// The investigator who moved.
        investigator: InvestigatorId,
        /// The location they entered.
        destination: LocationId,
    },
    /// See [`SlotDiscardFrame`].
    SlotDiscard(SlotDiscardFrame),
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
        /// Whose turn this is. Mirrors [`GameState::active_investigator`](crate::state::game_state::GameState::active_investigator) while on
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
    /// A parked enemy-attack loop: the attackers of one investigator, resolved
    /// one at a time (RR p.25 step 3.3). Since #704 the loop parks itself on
    /// this frame around **every** attack rather than only around a reaction
    /// window: the head attacker's `EnemyAttacks` condition walks the
    /// [`EmitEvent`](Self::EmitEvent) coordinator pushed above this frame, and
    /// the `drive` loop re-exposes this one when that coordinator pops. So the
    /// loop never reads the stack after emitting (ADR 0003) and an attack may
    /// suspend anywhere inside its own sequence.
    ///
    /// Never awaits player input at [`AttackLoopStage::Attacking`] — the
    /// coordinator above it owns any prompt; at
    /// [`AttackLoopStage::PickOrder`] it *is* the prompt. Lifted off the former
    /// `GameState::pending_enemy_attack` (#411, step 3 of #393).
    AttackLoop {
        /// The investigator whose engaged enemies are attacking.
        investigator: InvestigatorId,
        /// Attackers not yet resolved, in resolution order. At
        /// [`AttackLoopStage::Attacking`] the attacker whose sequence is running
        /// is still at the head — the loop removes it (and exhausts it) when the
        /// coordinator pops.
        remaining_attackers: Vec<EnemyId>,
        /// Which loop to re-enter.
        source: EnemyAttackSource,
        /// Where in the per-attacker sequence the loop stands (#704).
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
    /// One deal of damage and/or horror, in progress — the Rules Reference's
    /// two numbered steps and the window between them
    /// (`glossary/Dealing_Damage_Horror.md`), walked by the
    /// [`step`](DealDamageStep) cursor `Distribute → Announce → Place → Finish`.
    ///
    /// **This frame owns the live assignment**; each of the two emits snapshots
    /// it into its own event, so both events are true when emitted and there is
    /// no write-back protocol — `Place` simply re-reads the frame after
    /// `DamageAssigned`'s cells have had their chance to edit it. See
    /// `docs/adr/0009-damage-is-assigned-then-placed.md`.
    ///
    /// Awaits input only at [`Distribute`](DealDamageStep::Distribute), where it
    /// is the top frame and *is* the per-point prompt (resumed by
    /// `resume_damage_distribution`); at every other step the `drive` loop
    /// dispatches it the moment it is exposed.
    DealDamage {
        /// The investigator the harm is being dealt to.
        investigator: InvestigatorId,
        /// What is dealing it — the scoping both conditions carry, and how this
        /// frame resumes its caller at `Finish`.
        source: DamageSource,
        /// The live assignment: accumulating at `Distribute`, settled from
        /// `Announce` on.
        assignment: Assignment,
        /// Where in the two-step procedure this deal stands.
        step: DealDamageStep,
    },
    /// A node of an in-progress card-effect walk (#422). The effect evaluator is
    /// frame-driven: each control-flow node parks here while its children
    /// resolve; the global `drive` loop steps the top frame. Replaces the former
    /// single-pass replay (`DecisionCursor`). A node that needs a controller pick
    /// suspends *in place* (its `Leaf` step returns `AwaitingInput` and the frame
    /// stays on top — it *is* the prompt), so this variant can await input
    /// (routed in `resolve_input`, like [`Self::DealDamage`]). Carries its
    /// own [`EvalContext`] snapshot (#345's grouped
    /// bindings) so resume re-binds without replay.
    Effect(EffectFrame),
    /// The scenario's ending, in progress (#566). Pushed at the **bottom** of the
    /// stack by the engine's `end_scenario` the moment the resolution
    /// latches, so it is reached only once every frame above it has either
    /// completed or been cancelled
    /// ([`cancelled_by_scenario_end`](Self::cancelled_by_scenario_end)). It then
    /// emits the [`GameEnd`](crate::engine::TimingEvent::GameEnd) timing point —
    /// Cover Up 01007's *"Forced – When the game ends, if there are any clues on
    /// Cover Up: You suffer 1 mental trauma"* — whose forced abilities drain
    /// *above* it, possibly across an `apply` boundary (the interactive
    /// acknowledge). When it is exposed again at
    /// [`Finalize`](ScenarioEndStep::Finalize) the apply boundary — the only place
    /// holding the [`ScenarioRegistry`](crate::scenario::ScenarioRegistry) — pops
    /// it and runs the victory-display scan + the module's `apply_resolution`.
    ///
    /// That shape is also what makes `GameEnd` a **bare milestone**, and so
    /// coordinator-owned with a no-op resolve step (#720): the ending's teardown
    /// belongs to this frame's `Finalize` step, *after* the whole timing
    /// sequence, rather than to the condition. So the `when` cell is safe to
    /// walk, which is the cell Cover Up's *"When the game ends"* prints. See
    /// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
    ///
    /// Never awaits input (the acknowledge above it is the prompt); pushed once
    /// per scenario and popped once, so its presence *is* the once-only finalize
    /// marker: [`GameState::ending`](crate::state::game_state::GameState::ending) answers "did the scenario end", this
    /// frame answers "has the ending finished". See
    /// `docs/adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md`.
    ScenarioEnd {
        /// Where in the ending we are.
        step: ScenarioEndStep,
    },
    /// An investigator's elimination, in progress (#638). Pushed by
    /// `apply_investigator_elimination` **only** when the investigator owns an in-play
    /// weakness carrying a *"when the game ends"* Forced ability — Rules
    /// Reference p.10 Elimination **step 0**:
    ///
    /// > For the purpose of resolving weakness cards, the game has ended for the
    /// > eliminated investigator. Trigger any "when the game ends" abilities on
    /// > each weakness the eliminated investigator owns that is in play. Then,
    /// > remove those weaknesses from the game.
    ///
    /// Step 0's abilities must resolve **before** step 1 removes their cards
    /// (Cover Up 01007's *"Forced - When the game ends, if there are any clues
    /// on Cover Up: You suffer 1 mental trauma"* reads the clues still sitting
    /// on its own instance), and emitting a timing point only *queues* (ADR
    /// 0003) — so the remaining steps ride this frame while the queued abilities
    /// drain above it, and the loop re-exposes it to run steps 1–6.
    ///
    /// # The fork, and what it costs
    ///
    /// With no such weakness there is nothing to sequence, so
    /// `apply_investigator_elimination` runs the steps inline instead of pushing this
    /// frame. That is not merely an optimisation: it keeps the far commoner path
    /// — every elimination bar a Roland holding clues on Cover Up — reading a
    /// *finished* elimination, exactly as it did before #638.
    ///
    /// The two paths are **not** equivalent for a caller that resumes after
    /// `apply_investigator_elimination` returns. On this frame's path the investigator
    /// is already off `Status::Active`, but their cards are still in play and
    /// `check_all_eliminated` has not run — so no `AllInvestigatorsEliminated` and no
    /// `ScenarioEnding` latch yet. Post-defeat bookkeeping must therefore key
    /// off `Status`, never off a zone having been drained;
    /// `combat::place_assignment`'s asset sweep is the one such caller today and
    /// does exactly that.
    ///
    /// Same shape, same conclusion as [`ScenarioEnd`](Self::ScenarioEnd):
    /// `EliminationGameEnd` is a **bare milestone** too (#720). Steps 1–6 —
    /// including step 0's own *"Then, remove those weaknesses from the game"*
    /// tail — ride this frame and run at `RunSteps`, after the whole sequence,
    /// so they are not the condition's impact and its `when` cell is safe to
    /// walk. `apply_investigator_elimination`'s fork predicate must therefore ask
    /// about **every** cell: a hardcoded one drops a retagged ability silently,
    /// by taking the inline path and removing the weakness unfired.
    ///
    /// Never awaits input (the interactive acknowledge above it is the prompt),
    /// and never cancelled by a latched resolution: elimination is mandatory
    /// resolution already under way (ADR 0004).
    Elimination {
        /// The investigator being eliminated.
        investigator: InvestigatorId,
        /// Where in the elimination we are.
        step: EliminationStep,
    },
}

// One `impl_frame!` per newtype variant (#925), one line each, grouped by the
// ticket that converts the variant. Each group keeps its own header with a
// blank line before the next, so concurrent conversions edit disjoint lines;
// the headers can go once every variant is converted.

// Newtype variants before #928.
impl_frame!(SkillTest, InFlightSkillTest);
impl_frame!(HunterMove, HunterChoice);
impl_frame!(SpawnEngage, SpawnEngagePending);
impl_frame!(HandSizeDiscard, HandSizeDiscard);
impl_frame!(Effect, EffectFrame);

// Window and timing frames (#929).
impl_frame!(TimingPointWindow, TimingPointWindowFrame);
impl_frame!(FastWindow, FastWindowFrame);
impl_frame!(AcknowledgeForced, AcknowledgeForcedFrame);
impl_frame!(EmitEvent, EmitEventFrame);
impl_frame!(TimingPoint, TimingPointFrame);

// Phase, turn and action frames (#930).

// Draw, encounter and play frames (#931).
impl_frame!(SubstitutionPrompt, SubstitutionPromptFrame);
impl_frame!(Mulligan, MulliganFrame);
impl_frame!(EncounterDraw, EncounterDrawFrame);
impl_frame!(PlayerDraw, PlayerDrawFrame);
impl_frame!(EncounterCard, EncounterCardFrame);
impl_frame!(PlayFromHand, PlayFromHandFrame);
impl_frame!(SlotDiscard, SlotDiscardFrame);

// Combat, damage and resolution frames (#932).

/// Step cursor for the [`Elimination`](Continuation::Elimination) frame (#638).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EliminationStep {
    /// Step 0: emit the weakness-scoped game-end timing point. The `drive` loop
    /// advances the cursor *before* emitting — a tail-position emit, since the
    /// emit only queues (ADR 0003) and its frames must resolve above this one.
    FireWeaknessGameEnd,
    /// Step 0's abilities have drained. Run Elimination steps 1–6 (which remove
    /// those same weaknesses from the game) and pop the frame.
    RunSteps,
}

/// Step cursor for the [`ScenarioEnd`](Continuation::ScenarioEnd) frame (#566).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioEndStep {
    /// Emit the `GameEnd` timing point. The `drive` loop advances the cursor
    /// *before* emitting — a tail-position emit, since the emit only queues
    /// (ADR 0003) and its frames must resolve above this one.
    EmitGameEnd,
    /// The game-end forced abilities have drained. The apply boundary finalizes
    /// (victory display + `apply_resolution`) and pops the frame.
    Finalize,
}

/// How the framework disposes of a drawn encounter card after its Revelation's
/// whole sub-resolution completes (#423). Carried by
/// [`Continuation::EncounterCard`] so the unified disposal step
/// (`dispose_encounter_card_if_top`) handles both encounter card types from one
/// frame, regardless of which path revealed the card (engine-record draw,
/// Mythos chain, or an agenda reverse-draw — the last two having no
/// `EncounterDraw` frame to read the drawer from, so the enemy disposition
/// carries it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncounterDisposition {
    /// A treachery: discard to `encounter_discard` (or skip, if persistent — it
    /// placed itself during its Revelation).
    Discard,
    /// An enemy: spawn it into play at disposal, engaging the drawer
    /// (`investigator`) per the card's spawn instruction.
    Spawn {
        /// The drawing/controlling investigator — carried because the
        /// engine-record and agenda reverse-draw paths have no `EncounterDraw`
        /// frame beneath to read it from.
        investigator: InvestigatorId,
    },
}

/// One node of a frame-driven card-effect walk (#422). See
/// [`Continuation::Effect`]. Stepped by the evaluator's `step_effect_frame`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectFrame {
    /// A `Seq([..])` in progress: run `effects[next]`, advance `next` on each
    /// child pop, complete when `next == effects.len()`.
    Seq {
        /// The sequence's effects.
        effects: Vec<Effect>,
        /// Index of the next child to run.
        next: usize,
        /// The evaluation context for this sequence.
        ctx: EvalContext,
    },
    /// A single effect node to evaluate. A terminal effect runs and pops;
    /// `ChooseOne` pushes its chosen branch; `Effect::Deal` may push a
    /// `DealDamage` (K5b-2); `Effect::Native { tag }` runs the native fn.
    /// **Suspends in place** for a controller pick (`ChooseOne`, a `*::Chosen`
    /// target, a native choice): the step returns `AwaitingInput` and the frame
    /// stays on top — it *is* the prompt. Resume re-steps it with
    /// `ctx.chosen_option` set; the node grounds/picks (checked indexing,
    /// validate-first) instead of suspending.
    Leaf {
        /// The effect node to evaluate.
        effect: Box<Effect>,
        /// The evaluation context for this node.
        ctx: EvalContext,
    },
    /// The **designated action** of an activated ability, performed as the
    /// rules describe it but modified in the manner the ability carries
    /// (`glossary/Ability.md`; #805). Machete 01020's *"\[action\]:
    /// **Fight.**"* is this frame with a `+1 [combat]` modification, not an
    /// effect that happens to fight.
    ///
    /// A frame rather than a call inside the activation handler because
    /// choosing among 2+ co-located enemies **suspends in place** exactly as
    /// [`Leaf`](Self::Leaf) does, and re-enters on resume with
    /// `ctx.chosen_option` set — the same target-grounding machinery, on the
    /// same evaluation context, so a native predicate reading the chosen enemy
    /// (Machete's `sole_engaged_target`) binds identically either way.
    Designated {
        /// The bold action designator, carrying the ability's modification.
        designator: Box<ActionDesignator>,
        /// The evaluation context for the designated action.
        ctx: EvalContext,
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
    /// Resolve the activated ability for `source` after its `AoO` loop (#361):
    /// perform its designated action, then run its residual `effect`. Unlike
    /// the basic actions, this snapshots both rather than re-deriving them: an
    /// ability's declaration is fixed at activation (not board-dependent), and
    /// the source may have self-discarded as a cost (First Aid 01019 depleting
    /// its last supply), so a live re-resolution by source would be fragile.
    /// `source` is kept only as the eval context's source.
    ActivateAbility {
        /// The ability source — the eval context's `source` on resume (#707).
        source: AbilitySource,
        /// The bold action designator the ability prints, if any — what
        /// **performs the action** (#805), snapshotted at activation exactly as
        /// `effect` is. Flashlight 01087's **Investigate** is performed here,
        /// after the loop; a designated **Fight** or **Resign** never reaches
        /// this frame at all, being AoO-exempt.
        designator: Option<ActionDesignator>,
        /// The ability's residual effect, resolved at activation, run after the
        /// designated action. Empty for every ability implemented today.
        effect: Effect,
    },
    /// Complete a non-fast card play after its `AoO` loop (#378): run the card's
    /// `OnPlay` effects and, for an asset, move it into play. The card has
    /// already been announced (`CardPlayed`) and has left hand — it rides this
    /// frame across the whole `AoO` suspension, and its code re-derives the
    /// destination + `OnPlay` abilities from the registry on resume.
    PlayCard {
        /// The card mid-play — see [`Continuation::play_in_progress`].
        card: Option<CardCode>,
    },
}

impl Continuation {
    /// True if this is a `*Phase` anchor (slice 1b, #393): an
    /// [Inert](FrameActivity::Inert) framework frame the main loop's `drive`
    /// wakes when a child frame pops, never one that awaits player input
    /// itself.
    ///
    /// A *kind* predicate, not an activity: it is what the ADR 0003 check
    /// against [queued abilities](Self::is_queued_ability) keys on, and is
    /// separate from the [profile](Self::profile) for that reason.
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

    /// True if this frame is something a timing-point emit queued and the
    /// `drive` loop still owes resolution: the ability's effect frame, its
    /// interactive acknowledge, an open window / forced ordering run, or the
    /// `when → at → after` coordinator pair.
    ///
    /// Emitting a timing point queues rather than resolves (ADR 0003), so such a
    /// frame appearing *beneath* a [phase anchor](Self::is_phase_anchor) means a
    /// transition was pushed over work that had not happened yet — and since
    /// anchors pop-and-push rather than drain, that frame is stranded for the
    /// rest of the scenario (#569). [`ContinuationStack`]'s checked push
    /// debug-asserts against it.
    #[must_use]
    pub fn is_queued_ability(&self) -> bool {
        matches!(
            self,
            Continuation::Effect(_)
                | Continuation::AcknowledgeForced(_)
                | Continuation::TimingPointWindow(_)
                | Continuation::EmitEvent(_)
                | Continuation::TimingPoint(_)
                // Not queued *by* an emit — `apply_investigator_elimination` pushes
                // it — but it owes the loop the emit itself plus steps 1–6, and
                // burying it strands an elimination mid-sequence exactly as it
                // would strand an ability. Nothing can bury it today (an anchor
                // is only ever pushed with an anchor on top, i.e. beneath this
                // frame); it is included so that stays true by assertion rather
                // than by luck (#638).
                | Continuation::Elimination { .. }
        )
    }

    /// This frame's [`FrameProfile`]: how play treats it while it is the top
    /// frame, and what a latched scenario resolution does to it.
    ///
    /// **The one per-frame decision.** [`awaits_input`](Self::awaits_input),
    /// input routing's gate, the `drive` loop's idle arms and the scenario-end
    /// gate ([`cancelled_by_scenario_end`](Self::cancelled_by_scenario_end)) are
    /// all derived from it, so they cannot disagree. Exhaustive with no wildcard,
    /// so a new [`Continuation`] variant does not compile until it is classified.
    ///
    /// Computed from the frame's *value*, not just its variant: an event window
    /// is a prompt only while it has candidates, the open turn only while it is
    /// not ending, a deal of damage only while it distributes, an attack loop
    /// only at its order pick, and the ending frame rests inert at `Finalize`.
    ///
    /// The scenario-end disposition is ADR 0004's classification. Rules
    /// Reference: *"Some instructions in the act and agenda decks (as well as on
    /// other encounter cardtypes) contain resolution points, in the format of:
    /// '(→R#).' If a resolution point is reached, the scenario ends."* A frame
    /// that is an **opportunity to act** or part of the **framework sequence**
    /// is cancelled when it reaches the top; a frame that is **mandatory
    /// resolution already under way** completes. See
    /// `docs/adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md`.
    // One arm per variant (and per value-level split), each carrying the reason
    // for its classification; splitting it would scatter the one decision.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn profile(&self) -> FrameProfile {
        use FrameActivity::{Driven, Inert, Prompt};
        use ScenarioEndDisposition::{Cancel, Complete};
        let (activity, scenario_end) = match self {
            // An event window or forced run is the prompt while it has
            // candidates; empty, the `drive` loop closes it on sight (its
            // candidates are exhausted only by firing). A reaction window is an
            // opportunity the ended scenario cancels — act 01110's ruling that
            // its Forced objective "will trigger as soon as you defeat the Ghoul
            // Priest, before any 'After you defeat an enemy' reactions can be
            // used" is only honoured if Roland Banks' window never opens at all.
            // The forced run is the 2+-simultaneous half of the dispatch that
            // queues a lone forced ability's `AcknowledgeForced`, so the two
            // travel together as mandatory resolution.
            Continuation::TimingPointWindow(TimingPointWindowFrame {
                mode, candidates, ..
            }) => (
                if candidates.is_empty() {
                    Driven
                } else {
                    Prompt
                },
                match mode {
                    TimingMode::Reaction => Cancel,
                    TimingMode::Forced => Complete,
                },
            ),
            // Mandatory resolution that surfaces its own prompt (the `drive`
            // loop steps it until it does): the advance's flip acknowledge, the
            // forced acknowledge, the skill test's commit window and result
            // pause, a substitution choice, a slot make-room pick, an effect
            // node's controller pick.
            Continuation::AdvanceReverse { .. }
            | Continuation::AcknowledgeForced(_)
            | Continuation::SkillTest(_)
            | Continuation::SubstitutionPrompt(_)
            | Continuation::Effect(_)
            // Holds the asset mid-entry, in no zone (ADR 0002): discarding the
            // frame would leak the card out of every zone.
            | Continuation::SlotDiscard(_) => (Prompt, Complete),
            // A deal of damage is the per-point prompt while distributing a
            // contested point (#44/K5b); its other steps are sequencing the loop
            // dispatches on sight. Under way either way: half of it is the
            // placement, and abandoning it would leave an assignment that never
            // lands.
            Continuation::DealDamage { step, .. } => (
                match step {
                    DealDamageStep::Distribute { .. } => Prompt,
                    DealDamageStep::Announce | DealDamageStep::Place | DealDamageStep::Finish => {
                        Driven
                    }
                },
                Complete,
            ),
            // Framework prompts. Hunter movement and spawn engagement are board
            // maintenance whose prompt would otherwise be put to a player after
            // the game is over, and whose outcome no longer reaches state anyone
            // reads; the rest are the framework sequence, in which nothing
            // further in the round happens.
            //
            // A framework Fast window is a prompt with or without candidates:
            // `ResolveInput::Skip` closes it (#476), and the loop surfaces its
            // eligible plays as a skippable choice. An opportunity, so cancelled.
            Continuation::FastWindow(_)
            | Continuation::HunterMove(_)
            | Continuation::SpawnEngage(_)
            | Continuation::HandSizeDiscard(_)
            | Continuation::Mulligan(_)
            | Continuation::EncounterDraw(_) => (Prompt, Cancel),
            // The open turn surfaces its legal-action menu (2b, #447); once
            // `ending`, it is the rotation tail the loop drives after a
            // suspending `EndOfTurn` forced resolved.
            Continuation::InvestigatorTurn { ending, .. } => {
                (if *ending { Driven } else { Prompt }, Cancel)
            }
            // The attack loop is the attack-order pick at `PickOrder` (#143) and
            // otherwise re-exposed beneath the head attacker's coordinator, which
            // owns any prompt the attack raises (#704).
            Continuation::AttackLoop { stage, .. } => (
                match stage {
                    AttackLoopStage::PickOrder => Prompt,
                    AttackLoopStage::Attacking => Driven,
                },
                Cancel,
            ),
            // A surge chain is framework sequence; any prompt it opens (a
            // spawn-engagement tie) sits above it.
            Continuation::PlayerDraw(_) => (Driven, Cancel),
            // Internal sequencing that pushes the prompt above itself rather
            // than being it: the `when → at → after` coordinators, and the
            // frames awaiting the framework's disposal of a card in no zone (ADR
            // 0002) or the rest of an action already taken (ADR 0004 — half-
            // resolving it is harder to reason about than completing it).
            Continuation::EmitEvent(_)
            | Continuation::TimingPoint(_)
            | Continuation::EncounterCard(_)
            | Continuation::PlayFromHand(_)
            | Continuation::MoveEnter { .. }
            | Continuation::ActionResolution { .. }
            // An elimination under way: the acknowledge its step-0 emit queues
            // is the prompt, and steps 1–6 ask nothing. Rules Reference p.10 runs
            // its steps "any time a player is eliminated", and the weaknesses
            // whose game-end abilities drain above it are still in play until it
            // resumes (#638).
            | Continuation::Elimination { .. } => (Driven, Complete),
            // The ending emits `GameEnd` when driven, then rests at `Finalize`
            // for the apply boundary — the only place holding the scenario
            // registry — to pop (#566). It is the ending itself, so it completes.
            Continuation::ScenarioEnd { step } => (
                match step {
                    ScenarioEndStep::EmitGameEnd => Driven,
                    ScenarioEndStep::Finalize => Inert,
                },
                Complete,
            ),
            // Phase anchors wake only when a child frame pops; the framework
            // sequence they carry is over once the scenario has ended.
            Continuation::MythosPhase { .. }
            | Continuation::InvestigationPhase { .. }
            | Continuation::EnemyPhase { .. }
            | Continuation::UpkeepPhase { .. } => (Inert, Cancel),
        };
        FrameProfile {
            activity,
            scenario_end,
        }
    }

    /// True if a latched scenario resolution cancels this frame (#566): its
    /// [profile](Self::profile)'s disposition is
    /// [`Cancel`](ScenarioEndDisposition::Cancel).
    #[must_use]
    pub fn cancelled_by_scenario_end(&self) -> bool {
        self.profile().scenario_end == ScenarioEndDisposition::Cancel
    }

    /// True if this frame is a prompt: its [profile](Self::profile)'s activity
    /// is [`Prompt`](FrameActivity::Prompt), so it may rest on top at an `apply`
    /// boundary and `resolve_input` accepts a response for it.
    #[must_use]
    pub fn awaits_input(&self) -> bool {
        self.profile().activity == FrameActivity::Prompt
    }

    /// The resolution candidates of an open window/run on the stack —
    /// a [`TimingPointWindow`](Self::TimingPointWindow) (event windows + the
    /// #213 forced run) or a [`FastWindow`](Self::FastWindow) (framework
    /// windows). Lets the shared resolution driver read candidates without
    /// caring which window frame it is. `None` for any other frame.
    #[must_use]
    pub fn pending_candidates(&self) -> Option<&Vec<ResolutionCandidate>> {
        match self {
            Continuation::TimingPointWindow(TimingPointWindowFrame { candidates, .. })
            | Continuation::FastWindow(FastWindowFrame { candidates, .. }) => Some(candidates),
            _ => None,
        }
    }

    /// Mutable counterpart to [`Self::pending_candidates`].
    pub fn pending_candidates_mut(&mut self) -> Option<&mut Vec<ResolutionCandidate>> {
        match self {
            Continuation::TimingPointWindow(TimingPointWindowFrame { candidates, .. })
            | Continuation::FastWindow(FastWindowFrame { candidates, .. }) => Some(candidates),
            _ => None,
        }
    }

    /// The card this frame holds mid-play, with its controller, if any.
    ///
    /// Between Rules Reference Appendix I steps 3 and 4 a played card is in **no
    /// zone**: it has left hand ("the card commences being played") and is not
    /// yet "placed in play, or in its owner's discard pile if it's an event".
    /// The frame driving the play holds it for that stretch and hands it on —
    /// [`ActionResolution`](Self::ActionResolution) with an
    /// [`ActionResume::PlayCard`] across the attack-of-opportunity loop, then
    /// [`PlayFromHand`](Self::PlayFromHand) across the `OnPlay` effect, then
    /// [`SlotDiscard`](Self::SlotDiscard) across a make-room prompt. Exactly one
    /// frame holds a given card at a time, and the hand-off always pops the
    /// frame it came from.
    ///
    /// `None` for every other frame, and for a play frame whose card has already
    /// been taken — [`take_play_in_progress`](Self::take_play_in_progress) is the
    /// only taker that does not pop the frame. See
    /// `docs/adr/0002-in-progress-play-lives-on-its-frame.md`.
    #[must_use]
    pub fn play_in_progress(&self) -> Option<(InvestigatorId, &CardCode)> {
        match self {
            Continuation::ActionResolution {
                investigator,
                resume: ActionResume::PlayCard { card },
            }
            | Continuation::PlayFromHand(PlayFromHandFrame { investigator, card }) => {
                card.as_ref().map(|c| (*investigator, c))
            }
            // Same role, different payload: this frame holds the whole instance
            // (#772), so the code is read off it.
            Continuation::SlotDiscard(SlotDiscardFrame {
                investigator, card, ..
            }) => card.as_ref().map(|c| (*investigator, &c.code)),
            _ => None,
        }
    }

    /// Take `investigator`'s in-progress play off this frame, leaving the frame
    /// in place holding nothing. Returns `None` when the frame is not a play
    /// frame, belongs to someone else, or has already been emptied.
    ///
    /// Taking rather than copying is what keeps the frame's own disposal from
    /// placing a card that something else already placed. Two callers do so, and
    /// they reach different frames:
    ///
    /// - **elimination's sweep** walks *every* frame of the eliminated
    ///   investigator (Rules Reference p.10, step 1: their owned cards are
    ///   removed from the game — and a card mid-play is in no zone, so the
    ///   hand/deck/discard drain cannot reach it);
    /// - **`Effect::AttachSelfToLocation`** takes from one
    ///   [`PlayFromHand`](Self::PlayFromHand) frame only — its own (Barricade
    ///   01038 re-homing itself rather than discarding).
    ///
    /// So an [`ActionResolution`](Self::ActionResolution) or
    /// [`SlotDiscard`](Self::SlotDiscard) frame is emptied by elimination alone,
    /// which is what lets their resume paths treat "emptied" and "controller is
    /// no longer `Active`" as the same condition.
    ///
    /// **The card comes back with its owner**, because where a card in no zone
    /// is finally placed is a question about its owner rather than about the
    /// frame that was holding it (#772). A card being played from hand is the
    /// player's own; a card riding a [`SlotDiscard`](Self::SlotDiscard) frame
    /// may be one a [`TakeControl`](card_dsl::dsl::Effect::TakeControl) lifted off
    /// the board, and that one is the scenario's — `None`.
    pub fn take_play_in_progress(
        &mut self,
        investigator: InvestigatorId,
    ) -> Option<(CardCode, Option<InvestigatorId>)> {
        match self {
            Continuation::ActionResolution {
                investigator: owner,
                resume: ActionResume::PlayCard { card },
            }
            | Continuation::PlayFromHand(PlayFromHandFrame {
                investigator: owner,
                card,
            }) if *owner == investigator => card.take().map(|code| (code, Some(investigator))),
            Continuation::SlotDiscard(SlotDiscardFrame {
                investigator: owner,
                card,
                ..
            }) if *owner == investigator => card.take().map(|c| (c.code, c.owner)),
            _ => None,
        }
    }

    /// Take `investigator`'s cards-committed-to-a-skill-test off this frame,
    /// leaving the in-flight test holding none. Returns an empty `Vec` when the
    /// frame is not a [`SkillTest`](Self::SkillTest), belongs to someone else,
    /// or has already been emptied.
    ///
    /// The sibling of [`take_play_in_progress`](Self::take_play_in_progress),
    /// for the other card state that is in **no zone**: a card committed to a
    /// skill test is in limbo, "no longer considered to be in any
    /// investigator's hand" but not yet in a discard pile (Rules Reference
    /// glossary, "Limbo"), so elimination's hand/deck/discard drain cannot
    /// reach it either. Taking rather than copying keeps the ST.8 teardown from
    /// discarding a card that step 1 has already removed from the game — same
    /// order-independence argument as the play sweep (#604, #631).
    pub fn take_committed_cards(&mut self, investigator: InvestigatorId) -> Vec<CardCode> {
        match self {
            Continuation::SkillTest(t) if t.investigator == investigator => {
                mem::take(&mut t.committed_by_active)
            }
            _ => Vec::new(),
        }
    }

    /// Whether the frame is the mandatory #213 forced run. `false` for reaction
    /// windows and non-window frames.
    #[must_use]
    pub fn is_forced(&self) -> bool {
        matches!(
            self,
            Continuation::TimingPointWindow(TimingPointWindowFrame {
                mode: TimingMode::Forced,
                ..
            })
        )
    }

    /// The [`TimingEvent`] that opened this frame,
    /// if it is a [`TimingPointWindow`](Self::TimingPointWindow) (event window
    /// or forced run). `None` for [`FastWindow`](Self::FastWindow) framework
    /// windows (no timing event) and non-window frames. Lets the driver bind
    /// event-specific `EvalContext` (the attacking enemy, the would-be discovery
    /// count) directly from the timing event (#433).
    #[must_use]
    pub fn window_timing_event(&self) -> Option<&TimingEvent> {
        match self {
            Continuation::TimingPointWindow(TimingPointWindowFrame { event, .. }) => Some(event),
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
            Continuation::FastWindow(FastWindowFrame { fast_actors, .. }) => {
                fast_actors.permits(investigator)
            }
            Continuation::TimingPointWindow(TimingPointWindowFrame {
                mode: TimingMode::Reaction,
                ..
            }) => true,
            _ => false,
        }
    }
}

/// A frame's one classification — see [`Continuation::profile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameProfile {
    /// How play treats the frame while it is on top.
    pub activity: FrameActivity,
    /// What a latched scenario resolution does to the frame (ADR 0004).
    pub scenario_end: ScenarioEndDisposition,
}

/// How play treats a frame while it is the top of the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameActivity {
    /// The `drive` loop advances it; it never rests on top at an `apply`
    /// boundary, and `resolve_input` rejects it.
    Driven,
    /// It may rest on top at an `apply` boundary, and `resolve_input` accepts a
    /// response for it. The `drive` loop may still step it — the open turn
    /// surfaces its menu, a skill test advances to its commit window — but it is
    /// the one activity a frame can be waiting in when `apply` returns.
    Prompt,
    /// Neither the loop's step nor input advances it: a phase anchor, woken
    /// only when a child frame pops, and the ending frame at
    /// [`Finalize`](ScenarioEndStep::Finalize), which the apply boundary pops.
    Inert,
}

/// What a latched scenario resolution does to a frame when it reaches the top
/// (ADR 0004: a latched resolution cancels opportunities, not resolutions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioEndDisposition {
    /// An opportunity to act, or the framework sequence: discarded.
    Cancel,
    /// Mandatory resolution already under way: completes.
    Complete,
}

/// The Mythos-phase child-pop boundary an anchor resumes at (slice 1a, #393).
/// Names the framework window whose close re-enters the Mythos driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MythosResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening
    /// (round bump, `PhaseStarted`, steps 1.1–1.4) and replaces this with the
    /// running anchor.
    Entry,
    /// Step 1.1's `PhaseStarted { Mythos }` forced abilities are *queued* above
    /// this anchor (#697). On re-exposure, run steps 1.2/1.3 (place doom, check
    /// the threshold) and re-park at [`Draws`](Self::Draws). Set by
    /// `mythos_phase` before it emits, per ADR 0003.
    AfterPhaseStartForced,
    /// After step 1.2/1.3 (doom + agenda advance, incl. a suspending reverse)
    /// have resolved: run the step-1.4 encounter draws. `mythos_phase` parks the
    /// anchor here and the 1.4 draws run from `anchor_on_child_pop` once any
    /// `AdvanceReverse` frame above the anchor pops (#482).
    Draws,
    /// Post-step-1.4 (encounter draws done) window closed; run `mythos_phase_end`.
    AfterDraws,
    /// Step 1.5's `PhaseEnded { Mythos }` forced abilities are *queued* above
    /// this anchor (#697) — Wizard of the Order 01170 prints one. On
    /// re-exposure, run the Mythos → Investigation transition. Set by
    /// `mythos_phase_end` before it emits, per ADR 0003.
    AfterPhaseEndForced,
}

/// The Investigation-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvestigationResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Step 2.1's `PhaseStarted { Investigation }` forced abilities are *queued*
    /// above this anchor (#697). On re-exposure, open the post-2.1 player
    /// window and re-park at [`Begins`](Self::Begins). Set by
    /// `investigation_phase` before it emits, per ADR 0003.
    AfterPhaseStartForced,
    /// Post-2.1 window closed; begin the first investigator's turn.
    Begins,
    /// Post-2.2 turn-begins window closed; the investigator now acts (no
    /// continuation work — slice 2 makes this an `InvestigatorTurn` frame).
    TurnBegins,
    /// Step 2.3's `PhaseEnded { Investigation }` forced abilities are *queued*
    /// above this anchor (#697). On re-exposure, run the Investigation → Enemy
    /// transition. Set by `investigation_phase_end` before it emits, per ADR
    /// 0003.
    AfterPhaseEndForced,
}

/// The Enemy-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Step 3.1's `PhaseStarted { Enemy }` forced abilities are *queued* above
    /// this anchor (#697) — Dunwich's Hunting Horror 02141, Peter Clover 02079
    /// and agenda 02065 all print one. On re-exposure, run step 3.2 (hunter
    /// movement) and the step-3.3 attack-loop kickoff. Set by `enemy_phase`
    /// before it emits, per ADR 0003.
    AfterPhaseStartForced,
    /// Before-investigator-attacked window closed; resolve this investigator's
    /// attacks (step 3.3).
    BeforeInvestigatorAttacked,
    /// After-all-investigators-attacked window closed; run `enemy_phase_end`.
    AfterAllAttacked,
    /// Step 3.4's `PhaseEnded { Enemy }` forced abilities (agenda 01107's Ghoul
    /// move) are *queued* above this anchor. On re-exposure — once those frames
    /// have resolved — run the Enemy → Upkeep transition. Set by
    /// `enemy_phase_end` before it emits, so the transition can never be pushed
    /// on top of an ability the emit just queued (#569, ADR 0003).
    AfterPhaseEndForced,
}

/// The Upkeep-phase child-pop boundary (slice 1a, #393).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpkeepResume {
    /// Just entered (slice 1b, #393): the loop's `advance` runs the phase opening.
    Entry,
    /// Step 4.1's `PhaseStarted { Upkeep }` forced abilities are *queued* above
    /// this anchor (#697). On re-exposure, open the post-4.1 player window and
    /// re-park at [`Begins`](Self::Begins). Set by `upkeep_phase` before it
    /// emits, per ADR 0003.
    AfterPhaseStartForced,
    /// Post-4.1 window closed; run `upkeep_resume` (steps 4.2–4.6).
    Begins,
    /// Steps 4.2–4.4 ran and the 4.4 draw pushed a drawn-weakness Revelation
    /// (#509) that ceded to the drive loop. On re-exposure of the anchor, run
    /// 4.5 (hand size) + 4.6 (phase end).
    AfterDraw,
    /// Step 4.6's `PhaseEnded { Upkeep }` forced abilities are *queued* above
    /// this anchor. On re-exposure — once those frames have resolved — emit
    /// `RoundEnded` (the mirror of [`EnemyResume::AfterPhaseEndForced`], #569).
    /// The round-end emit lives in this arm rather than inline after the
    /// phase-end emit precisely because the latter queues rather than resolves.
    AfterPhaseEndForced,
    /// The round-end `EmitEvent` coordinator (the `when` act advance + the `at`
    /// doom) popped; run `upkeep_round_end_teardown` (expire until-end-of-round
    /// effects, Upkeep → Mythos). Set before the coordinator is queued
    /// (#434 — subsumes `ForcedContinuation::UpkeepAfterRoundEnded`).
    AfterRoundEnd,
}

/// The cursor of a [`Continuation::EmitEvent`] walk: one triggering condition's
/// timing sequence (#701).
///
/// The Rules Reference puts the condition's own resolution *inside* the
/// sequence — `glossary/Nested_Sequences.md`: *"Each time a triggering condition
/// occurs, the following sequence is followed: 1) execute "when..." effects that
/// interrupt that triggering condition, (2) resolve the triggering condition,
/// and then, (3) execute "after..." effects in response to that triggering
/// condition."* [`ResolveCondition`](Self::ResolveCondition) is that step 2; the
/// `At` cell sits between it and `After` per `glossary/At.md`.
///
/// Who performs step 2 is the engine-internal `ConditionResolution` classification,
/// dispatched from the timing event's own value — the frame is serialized, so
/// the step cannot hold a closure. See
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitStep {
    /// The `when` cell: abilities that interrupt the condition, resolving
    /// before its impact lands. Walked only for a coordinator-owned condition —
    /// a caller-owned one has already mutated, so an interrupt here would
    /// resolve *after* the thing it meant to interrupt.
    When,
    /// Step 2: resolve the triggering condition itself. A no-op for a
    /// caller-owned condition (the emitting caller already mutated).
    ResolveCondition,
    /// The `at` cell: `at …` / `if …` abilities, between the when and after
    /// cells.
    At,
    /// The `after` cell: abilities responding to the fully-resolved condition.
    After,
}

impl EmitStep {
    /// The timing cell this step scans for abilities, or `None` for the
    /// resolve step (which scans nothing — it resolves the condition).
    #[must_use]
    pub fn cell(self) -> Option<EventTiming> {
        match self {
            EmitStep::When => Some(EventTiming::When),
            EmitStep::ResolveCondition => None,
            EmitStep::At => Some(EventTiming::At),
            EmitStep::After => Some(EventTiming::After),
        }
    }

    /// The next step in the sequence, or `None` once `After` is done (the
    /// coordinator pops).
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self {
            EmitStep::When => Some(EmitStep::ResolveCondition),
            EmitStep::ResolveCondition => Some(EmitStep::At),
            EmitStep::At => Some(EmitStep::After),
            EmitStep::After => None,
        }
    }

    /// Every cell of the sequence, in order: the cursor walked from its start,
    /// keeping the steps that are cells.
    ///
    /// For a caller asking about the **whole sequence** rather than about one
    /// cell — *"does this investigator own a weakness with any game-end
    /// ability?"* — where scanning a single hardcoded cell answers a different
    /// question. That mistake does not reject; it drops the ability silently,
    /// which is why #720 hit it in `has_weakness_game_end_ability` and #723 hit
    /// it in three `test_support` helpers.
    ///
    /// **Derived, not listed.** A fourth cell has to be an `EmitStep` before the
    /// coordinator can walk it at all, and both [`cell`](Self::cell) and
    /// [`next`](Self::next) are exhaustive matches — so it arrives here on its
    /// own rather than waiting to be remembered. That is the same reason ADR
    /// 0008 keeps `ConditionResolution` an exhaustive match instead of a table:
    /// *a table is a thing a new variant can be forgotten from*.
    pub fn cells() -> impl Iterator<Item = EventTiming> {
        iter::successors(Some(EmitStep::When), |step| step.next()).filter_map(EmitStep::cell)
    }
}

/// The forced-then-reaction sub-cursor of a [`Continuation::TimingPoint`]
/// (#434). `Forced` fires the bucket's forced abilities (0/1 inline, 2+ via the
/// lead-ordered run), `Reaction` opens the bucket's reaction window, `Done`
/// finishes the bucket (advance the parent `EmitEvent`'s cursor + pop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingSub {
    /// Fire the bucket's forced abilities.
    Forced,
    /// Open the bucket's reaction window.
    Reaction,
    /// Bucket resolved; advance the parent `EmitEvent` cursor and pop.
    Done,
}

/// A skill test paused mid-resolution at the commit window.
///
/// Pushed by the skill-test initiator (a plain skill test, `Investigate`,
/// `Fight`, `Evade`) after [`SkillTestStarted`] fires; consumed by the
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) dispatch
/// once the active investigator submits their commit list. The follow-
/// up describes the action-specific success path: discover a clue
/// (Investigate), deal damage (Fight), disengage and exhaust (Evade),
/// or nothing (a bare plain skill test).
///
/// [`SkillTestStarted`]: crate::event::Event::SkillTestStarted
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InFlightSkillTest {
    /// This test's identity, minted from
    /// [`GameState::skill_test_ids`](crate::state::game_state::GameState::skill_test_ids) at initiation.
    ///
    /// What a test-scoped [`RecordedModifier`](crate::state::game_state::RecordedModifier) is stamped with, and what its
    /// expiry sweep matches on at teardown — so a buff bought "for this
    /// skill test" applies to *this* one and to no other. Monotonic and
    /// never reused, so a row that outlived its test (it cannot today, but
    /// the guarantee is what makes that structural rather than a matter of
    /// draining diligently) is inert rather than misattributed.
    pub id: SkillTestId,
    /// Investigator taking the test.
    pub investigator: InvestigatorId,
    /// Skill the test is against.
    pub skill: SkillKind,
    /// Test kind (Investigate / Fight / Evade / Plain). Drives
    /// [`ModifierScope::WhileInPlayDuring`](card_dsl::dsl::ModifierScope::WhileInPlayDuring)
    /// matching during resolution.
    pub kind: SkillTestKind,
    /// What this test's difficulty is read *from* — never a number
    /// computed when the test started.
    ///
    /// The difficulty is a query —
    /// [`ModifiedQuantity::Difficulty`](crate::engine::modified_value::ModifiedQuantity::Difficulty)
    /// over [`ModifierTarget::Test`](crate::state::game_state::ModifierTarget::Test), which resolves this basis and reads
    /// the underlying location's shroud or enemy's fight/evade as the board
    /// stands at ST.6 (#677). [`DifficultyBasis`] carries the rule.
    pub difficulty_basis: DifficultyBasis,
    /// The cards the active investigator has committed to the test, in
    /// commit order — **held here, not in hand**. Rules Reference glossary
    /// "Limbo": *"A skill card enters limbo as it is committed to a skill
    /// test. … It is no longer considered to be in any investigator's hand,
    /// but it has not yet been placed in any discard pile."*
    ///
    /// Populated on the [`ResolveInput`](crate::action::PlayerAction::ResolveInput)
    /// dispatch, which removes the named cards from the hand and moves them
    /// here; the ST.8 teardown moves them on to the discard pile. **Load-
    /// bearing for resolution**: the frame-driven steps read the committed
    /// codes off this field (e.g. `collect_on_skill_test_resolution` /
    /// `discard_committed_cards` via `in_flight.committed_by_active`), so it
    /// must stay accurate across suspensions — don't treat it as
    /// inspection-only metadata.
    ///
    /// Storing the codes rather than the hand positions they came from is
    /// what makes the ST.2→ST.3 player window safe (#631): a Fast play
    /// resolved inside that window mutates the hand, and any parked hand
    /// index would then denote a different card (or none at all).
    ///
    /// Multi-investigator commits (the rule "any investigator at the
    /// same location may commit") are a separate downstream issue; for
    /// now only the active investigator's commits live here.
    pub committed_by_active: Vec<CardCode>,
    /// The location the test is associated with, snapshotted at
    /// skill-test start (`engine::dispatch::start_skill_test`) from
    /// the investigator's current location. Used by
    /// [`LocationTarget::TestedLocation`](card_dsl::dsl::LocationTarget::TestedLocation)
    /// during
    /// [`Trigger::OnSkillTestResolution`](card_dsl::dsl::Trigger::OnSkillTestResolution)
    /// firing so "at that location" resolves to the location the
    /// test was originally taken against, even if the investigator
    /// has since moved (no Phase-3 path moves mid-test, but the
    /// snapshot future-proofs against cards that will). `None` when
    /// the investigator was between locations at test start —
    /// only reachable via a bare plain skill test (the
    /// [`test_support::perform_skill_test`](crate::test_support::perform_skill_test)
    /// synthetic entry point) from outside an Investigate path.
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
    pub on_fail: Option<Effect>,
    /// Effect to run **on success** after the chaos token resolves (the
    /// success-side mirror of [`on_fail`](Self::on_fail)). Carried by
    /// `Effect::SkillTest` with a success branch — Frozen in Fear 01164's
    /// end-of-turn willpower test discards the card on success. `None` for
    /// action tests and failure-only card tests.
    pub on_success: Option<Effect>,
    /// The firing ability's source, parked so the `on_success` / `on_fail`
    /// eval-contexts are rebuilt with it after the suspension. `None` for basic
    /// action tests and for effects with no originating source.
    ///
    /// The whole [`AbilitySource`] rides here, not its
    /// [`instance`](AbilitySource::instance) projection, because the anchor
    /// would otherwise be destroyed at exactly this boundary: an act's
    /// `on_fail: ChooseOne` would be anchored before the chaos draw and
    /// un-anchored after it (#834). The projection is what
    /// [`Effect::DiscardSelf`] reads back
    /// out to find itself.
    pub source: Option<AbilitySource>,
    /// Where the resolution driver should resume on the next call to
    /// `advance`. Initialized to
    /// [`SkillTestStep::AwaitingCommit`] at
    /// `start_skill_test`; advanced in lock-step as the resolution
    /// sequence runs. The test outcome lives on [`resolved`](Self::resolved)
    /// (set at ST.5–ST.6, one step later than the commit window closes — see
    /// that field), not in the cursor payloads.
    pub continuation: SkillTestStep,
    /// Bonus damage added to this attack, accumulated at commit time by
    /// [`Effect::BoostAttackDamage`]
    /// (Vicious Blow 01025). Read **only** by the `Fight` follow-up, which
    /// deals `1 + extra_damage + bonus_attack_damage` on success — so it
    /// is inert for non-Fight tests. `0` for every test that no
    /// commit-time attack buff touches (regression-safe).
    pub bonus_attack_damage: u8,
    /// Bonus clues added to this investigation's discovery, accumulated at
    /// commit time by
    /// [`Effect::DiscoverAdditionalClues`]
    /// (Deduction 01039). Read **only** by the `Investigate` follow-up, which
    /// makes **one** discovery of `1 + bonus_clues_discovered` on success — so
    /// it is inert for non-Investigate tests. The sibling of
    /// [`bonus_attack_damage`](Self::bonus_attack_damage); `0` for every test
    /// no commit-time clue buff touches.
    ///
    /// Raising one discovery's count is not interchangeable with adding a
    /// second discovery: Cover Up 01007 replaces one discovery of 2, not two
    /// of 1 (#471). See the **Discovery** entry in `GLOSSARY.md`.
    pub bonus_clues_discovered: u8,
    /// The test's determination, set once at the
    /// [`DetermineOutcome`](SkillTestStep::DetermineOutcome) step (RR ST.5–ST.6)
    /// and read by every later step instead of threading `succeeded`/`failed_by`
    /// through each cursor variant. `None` until the test resolves — so
    /// `resolved.is_some()` is the structural witness for "the test is past
    /// [`DetermineOutcome`](SkillTestStep::DetermineOutcome)". (Slice D #423;
    /// moved from `Resolving` to `DetermineOutcome` by #674, so the ST.5 sum
    /// sees the board the ST.4 chaos-symbol effects left behind.)
    pub resolved: Option<ResolvedTest>,
    /// A chaos symbol token's result-conditional `on_fail` effect (Cultist
    /// 01104's "if this test is failed, take 1 horror"), built at the
    /// `Resolving` step and pushed at the `ApplySymbolOnFail` step (RR ST.7,
    /// *after* the outcome timing point). `None` when the test passed or the
    /// symbol has no `on_fail`. Held here (a sibling of [`on_fail`](Self::on_fail)
    /// / [`on_success`](Self::on_success)) because it is a non-`Copy` `Effect`
    /// needed several steps after the token is drawn. (Slice D #423.)
    pub symbol_on_fail: Option<Effect>,
}

/// The outcome of a skill test's chaos-token resolution (RR ST.6), stored on
/// [`InFlightSkillTest::resolved`] once computed and read by every subsequent
/// driver step. One source of truth, rather than routing the same fields
/// through each [`SkillTestStep`] payload. `Copy` (all scalar fields) so the
/// driver reads it cheaply; the result-conditional symbol `on_fail` *effect*
/// lives separately on [`InFlightSkillTest::symbol_on_fail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTest {
    /// Whether the test passed: the test's determination where it has one,
    /// and `total >= difficulty` otherwise.
    pub succeeded: bool,
    /// Failure margin (`difficulty - total`, clamped ≥ 0); `0` on success.
    /// Read by `IntExpr::Count(Quantity::SkillTestFailedBy)` in an `on_fail`
    /// effect (Grasping Hands 01162, Rotting Remains 01163).
    pub failed_by: u8,
    /// Success margin (`total - difficulty`, ≥ 0 on success); supplied to the
    /// logged [`SkillTestSucceeded`](crate::event::Event::SkillTestSucceeded) at the
    /// `DetermineOutcome` step. Negative on failure (unused there).
    pub margin: i8,
    /// Why the test failed (meaningful only when `!succeeded`); supplied to the
    /// logged [`SkillTestFailed`](crate::event::Event::SkillTestFailed).
    pub fail_reason: FailureReason,
}

/// Where the skill-test resolution driver should resume on the next
/// call to `advance`.
///
/// The driver (`advance`, with the resolution body in `run_resolution`)
/// walks a fixed sequence of steps:
///
/// 1. Validate commits + draw chaos token + emit
///    [`SkillTestSucceeded`](crate::event::Event::SkillTestSucceeded) /
///    [`SkillTestFailed`](crate::event::Event::SkillTestFailed)
/// 2. Apply the action-specific
///    [`SkillTestFollowUp`] (Investigate / Fight / Evade / None) —
///    this is where `damage_enemy` may emit
///    [`EnemyDefeated`](crate::event::Event::EnemyDefeated) and queue an
///    an after-enemy-defeated reaction window
/// 3. Fire
///    [`OnSkillTestResolution`](card_dsl::dsl::Trigger::OnSkillTestResolution)
///    triggers on committed cards
/// 4. Discard committed cards + emit
///    [`SkillTestEnded`](crate::event::Event::SkillTestEnded) + drain
///    pending modifiers
///
/// After each step that *can* queue a reaction window, the driver checks
/// whether that window is now the top frame; if so it suspends with
/// [`AwaitingInput`](crate::engine::EngineOutcome::AwaitingInput) and yields to the
/// `drive` loop, which dispatches the window. On the window's close the loop
/// re-dispatches this `SkillTest` frame, which reads its cursor and jumps to
/// the matching step (Slice C-plumbing). This is the rules-correct shape per
/// the Rules Reference's "after… initiates immediately after that
/// triggering condition's impact has resolved" clause: the reaction
/// fires between steps 2 and 3, not after the entire action ends.
///
/// The test outcome (`succeeded`/`failed_by`/`margin`/`fail_reason`) is
/// determined once at [`DetermineOutcome`](Self::DetermineOutcome) (ST.5–ST.6)
/// and stored on [`InFlightSkillTest::resolved`]; every subsequent step reads it
/// from there rather than carrying it in the cursor. `resolved.is_some()` is the
/// witness for "the test is past `DetermineOutcome`."
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
///   commit is validated and stored. The next driver iteration runs the
///   computation body (`run_resolution`: ST.3–ST.6, pushing the ST.4 immediate
///   symbol effects) and pre-advances to
///   [`DetermineOutcome`](Self::DetermineOutcome).
/// - [`DetermineOutcome`](Self::DetermineOutcome) — the ST.6→ST.7 boundary:
///   emit the logged success/failure events, then the `SkillTestResolved`
///   timing point, **before** any ST.7 consequence resolves.
/// - [`ApplyFollowUp`](Self::ApplyFollowUp) /
///   [`ApplyResultEffect`](Self::ApplyResultEffect) — the ST.7 "apply results"
///   sub-steps: action follow-up (the clue discovery), then the
///   success/failure card effect. Each effect is pushed for the global drive
///   loop and cursor-sequenced so results resolve in ST order.
/// - [`FireOnResolution`](Self::FireOnResolution) — fire the committed cards'
///   `OnSkillTestResolution` triggers, one per visit.
/// - [`PostOnResolution`](Self::PostOnResolution) — terminal teardown (ST.8).
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
    /// Commit submitted: the next driver iteration reveals the chaos token
    /// (RR ST.3) and pushes the symbol's immediate effects (RR ST.4), then
    /// pre-advances to [`DetermineOutcome`](Self::DetermineOutcome). It records
    /// the token's modifier (or the elder sign's expression) as a
    /// [`RecordedModifier`](crate::state::game_state::RecordedModifier) scoped to this test — an `[auto_fail]` records a
    /// [`Determination`](card_dsl::dsl::Determination) instead — and computes
    /// **no** total, margin, or verdict: those are ST.5/ST.6 and belong after
    /// the symbol effects have resolved (#674, #684, #685).
    ///
    /// **The reveal is conditional (#687).** A determination already latched
    /// when the driver arrives here skips ST.3 and ST.4 outright — *"No chaos
    /// token(s) are revealed from the chaos bag, and the investigator
    /// immediately moves to Step 5"* (`glossary/Automatic_Failure_Success.md`)
    /// — while the `[auto_fail]` token's own determination, latched *at* this
    /// step, skips nothing, per the same entry. Both clauses are the one
    /// question asked at two moments (ADR 0007); the driver arm is documented
    /// on `dispatch::skill_test`.
    Resolving,
    /// RR ST.5–ST.7 boundary. Sum the modified skill value (ST.5) from the
    /// board as it stands *now* — after the ST.4 symbol effects — compare it
    /// against the difficulty (ST.6), then emit the logged
    /// [`SkillTestSucceeded`](crate::event::Event::SkillTestSucceeded) /
    /// [`SkillTestFailed`](crate::event::Event::SkillTestFailed) and fire the general
    /// skill-test-outcome timing point (`TimingEvent::SkillTestResolved`) for
    /// **every** test and both outcomes — "after you successfully investigate"
    /// (Obscuring Fog 01168 forced + Dr. Milan 01033 reaction) is the
    /// `{ Investigate, Success }` narrowing. Fires before any ST.7 consequence;
    /// an empty forced/reaction candidate set opens no window. Reads the outcome
    /// off [`InFlightSkillTest::resolved`]; pre-advances to
    /// [`FireOnCommit`](Self::FireOnCommit). (Slice D #423.)
    ///
    /// **Firing here rather than at ST.7 is the published rule, twice over.**
    /// `data/official-faq/Rulings_and_Clarifications.md`, section 1.7:
    ///
    /// > \[reaction\] or Forced abilities with a triggering condition dependent
    /// > upon the skill test being successful or unsuccessful (such as "After
    /// > you successfully investigate," or "After you fail a skill test by 2 or
    /// > more") do not trigger at this time. These abilities are triggered
    /// > during Step 6, "Determine success/failure of skill test."
    ///
    /// ("At this time" is Step 7.) The FAQ's Q&A works the same case in the
    /// other direction, separating what belongs to this step from what belongs
    /// to ST.7: *"The effects of a successful skill test are applied during
    /// step 7 \[…\] Dr. Milan Christopher's ability is a reaction to
    /// succeeding at a skill test, and therefore is triggered and resolved
    /// during step 6, after success is determined."*
    /// (`data/official-faq/Frequently_Asked_Questions.md`.) So Obscuring Fog
    /// 01168 and Dr. Milan 01033 resolve **before** the clue moves, and it is
    /// the ST.7 half — the discovery itself, and any *"if this test is
    /// successful"* clause — that a doubling effect would double.
    DetermineOutcome,
    /// Cosmetic acknowledgment pause (#478). The result events
    /// (`ChaosTokenRevealed`, `SkillTestSucceeded`/`Failed`) are already emitted
    /// at [`DetermineOutcome`](Self::DetermineOutcome); when
    /// [`GameState::interactive_acknowledge`](crate::state::GameState::interactive_acknowledge)
    /// is set, `advance` suspends here with an `AwaitingInput { InputKind::Confirm }`
    /// so an interactive host can show the player the result before the ST.7
    /// consequence resolves. The cursor stays here across the suspension;
    /// `acknowledge_outcome` advances it to [`FireOnCommit`](Self::FireOnCommit)
    /// on the Confirm resume (mirroring the `AwaitingCommit` / `finish_skill_test`
    /// handshake). When the flag is off, `advance` advances straight to
    /// `FireOnCommit` without pausing.
    AcknowledgeOutcome,
    /// RR ST.7 head — push the committed cards' [`Trigger::OnCommit`] effects
    /// (Vicious Blow 01025's `BoostAttackDamage`). These are conditional on
    /// success ("If this skill test is successful during an attack…") so they
    /// belong after the token is resolved, but **before**
    /// [`ApplyFollowUp`](Self::ApplyFollowUp) reads the
    /// `bonus_attack_damage` accumulator they populate. Collected into one
    /// [`Effect::Seq`] and pushed for the drive loop
    /// (nothing pushed if no committed card carries an `OnCommit` trigger);
    /// pre-advances to [`ApplyFollowUp`](Self::ApplyFollowUp).
    ///
    /// [`Trigger::OnCommit`]: card_dsl::dsl::Trigger::OnCommit
    FireOnCommit,
    /// RR ST.7 part 1 — apply the action-specific
    /// [`SkillTestFollowUp`]. On success the Investigate follow-up
    /// pushes its `discover_clue` effect for the drive loop (yielding);
    /// Fight / Evade / None run synchronously. On failure the follow-up
    /// is skipped (follow-ups are success-only). Pre-advances to
    /// [`ApplyResultEffect`](Self::ApplyResultEffect). (Slice D #423.)
    ApplyFollowUp,
    /// RR ST.7 part 2 — push the success/failure card effect: `on_success`
    /// on a passing draw (Frozen in Fear 01164's self-discard), or `on_fail`
    /// on a failing draw (Crypt Chill 01167's discard choice, Grasping Hands
    /// 01162's margin damage). Exactly one (or neither) is pushed; it runs
    /// after the follow-up because this step is sequenced after
    /// [`ApplyFollowUp`](Self::ApplyFollowUp). Pre-advances to
    /// [`ApplySymbolOnFail`](Self::ApplySymbolOnFail). (Slice D #423.)
    ApplyResultEffect,
    /// RR ST.7 — push a chaos symbol token's result-conditional `on_fail`
    /// effect (Cultist 01104's horror), held on
    /// [`InFlightSkillTest::symbol_on_fail`], when the test failed. Sits among
    /// the ST.7 result effects (after the card `on_fail` of
    /// [`ApplyResultEffect`](Self::ApplyResultEffect)); RR lets the test-performer
    /// order multiple results, the engine sequences deterministically. Pushed
    /// via [`Effect::Deal`] so a sanity-soak (Holy
    /// Rosary 01028) suspends cleanly. Pre-advances to
    /// [`FireOnResolution`](Self::FireOnResolution). (Slice D #423.)
    ApplySymbolOnFail,
    /// RR ST.7 — fire the committed cards' [`OnSkillTestResolution`] triggers,
    /// one effect per driver visit so they cursor-sequence in committed-card
    /// order (no LIFO). `next` is the index into the flattened
    /// (card, matching-ability) list of the next trigger to fire; each visit
    /// pushes that effect, advances `next`, and yields. When `next` runs past
    /// the list, advances to [`PostRetaliate`](Self::PostRetaliate). Replaces
    /// the former single-shot `PostFollowUp` step. (Slice D #423.)
    ///
    /// The test outcome (`succeeded`/`failed_by`) is read off
    /// [`InFlightSkillTest::resolved`] rather than carried in the cursor — `next`
    /// is the only step-specific state this variant needs.
    ///
    /// [`OnSkillTestResolution`]: card_dsl::dsl::Trigger::OnSkillTestResolution
    FireOnResolution {
        /// Index of the next matching (card, ability) trigger to fire.
        next: u32,
    },
    /// Step 3 (`OnSkillTestResolution`) is complete. The next driver
    /// iteration fires a Retaliate attack if the test was a failed Fight
    /// against a ready retaliate enemy (Rules Reference p.18 — "after
    /// applying all results for that skill test"), then advances to
    /// teardown.
    PostRetaliate,
    /// Step 3 (`OnSkillTestResolution`) is complete. The next driver
    /// iteration discards committed cards, emits
    /// [`SkillTestEnded`](crate::event::Event::SkillTestEnded), and clears
    /// the in-flight record.
    PostOnResolution,
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
    /// No action-specific follow-up. Used by a bare plain skill test (the
    /// [`test_support::perform_skill_test`](crate::test_support::perform_skill_test)
    /// synthetic entry point).
    None,
    /// On success, make **one** discovery of
    /// `1 + `[`bonus_clues_discovered`](InFlightSkillTest::bonus_clues_discovered)
    /// clues at the test's
    /// [`tested_location`](InFlightSkillTest::tested_location) (via the
    /// [`DiscoverClue`](card_dsl::dsl::Effect::DiscoverClue) evaluator path).
    /// Used by `Investigate`. A commit-time "discover 1 additional clue"
    /// (Deduction 01039) raises this discovery's count rather than adding a
    /// second discovery — see the **Discovery** entry in `GLOSSARY.md`.
    Investigate,
    /// On success, deal 1 damage to the named enemy (and defeat it if
    /// damage reaches `max_health`). Used by
    /// `Fight`.
    Fight {
        /// The enemy the Fight action targeted.
        enemy: EnemyId,
        /// Bonus damage beyond the base 1 (weapons). `0` for a basic Fight.
        extra_damage: u8,
    },
    /// On success, disengage the named enemy from the investigator and
    /// exhaust it. Used by `Evade`.
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
    Specific(BTreeSet<InvestigatorId>),
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

/// Whether a [`TimingPointWindow`](Continuation::TimingPointWindow) is a
/// skippable reaction window or the mandatory #213 forced run. Collapses the
/// old `ResolutionKind::{Window | Forced}` split onto the one frame: a forced
/// run admits no Fast plays and drains all candidates. It carries **no** resume
/// continuation (#434): on close it returns `Done` and the `drive` loop
/// re-dispatches whatever frame is exposed beneath it (the coordinator's
/// `TimingPoint`, the `InvestigatorTurn { ending }` frame, the move's
/// `ActionResolution`, …). The invariant is that any emit site capable of a
/// 2+-forced run resumes via its own frame — see the deleted
/// `ForcedContinuation`'s former call sites (#434).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingMode {
    /// A reaction/fast window: skippable, admits Fast plays.
    Reaction,
    /// The forced run (#213): mandatory, no Fast plays. Carries no resume
    /// continuation; the loop re-dispatches the exposed parent frame on close.
    Forced,
}

/// The Rules-Reference timing step a [`FastWindowKind::Phase`] window sits
/// at. Each step uniquely determines its phase, so the phase is not
/// carried separately (the engine reads [`GameState::phase`](crate::state::game_state::GameState::phase)).
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
    /// [`turn_order`]: crate::state::game_state::GameState::turn_order
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
/// `PickSingle`.
///
/// Distinct from [`HunterChoice`] because spawn engagement is not a
/// hunter move (it never picks a location) and its resume just engages
/// the chosen investigator and pops; any Mythos encounter-draw chain
/// continues through the [`PlayerDraw`](Continuation::PlayerDraw) frame
/// beneath it (which carries its own surge/chain state), so this frame
/// holds only the pick itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpawnEngagePending {
    /// The spawned enemy awaiting an engagement target.
    pub enemy: EnemyId,
    /// Co-located investigators to choose among.
    pub candidates: Vec<InvestigatorId>,
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
/// An [`Ability`](Self::Ability) candidate **fires an ability's effect** in
/// place, from the [`AbilitySource`] it names; a [`Hand`](Self::Hand) candidate
/// (Axis C, #335) is a Fast event **played** from hand (RR Appendix I —
/// `CardPlayed`, run the matched ability's effect, discard), not fired in
/// place. That is the whole of the distinction, which is why the firing half is
/// not a set of kinds of its own: **where an ability comes from is
/// [`AbilitySource`]**, the same vocabulary an activation names (#735, ADR
/// 0010).
///
/// The two descriptors used to be parallel enums that had drifted apart —
/// `CandidateSource::Board` covered the act, the agenda *and* an attacking
/// enemy's own ability with one kind, so every reader had to re-derive which
/// board card it was by comparing the candidate's code against the current act
/// and agenda. Wrapping instead of merging is what keeps `Hand` out of
/// `AbilitySource`: a card played from hand is **not** an ability source in the
/// rules' sense (`glossary/Triggered_Abilities.md` lists sources of *triggered
/// abilities*), and `AbilitySource` is what the reachability predicate
/// enumerates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CandidateSource {
    /// An ability fired in place, from the source that carries it: an in-play
    /// instance (reaction trigger, weapon, the investigator card), a location
    /// (the Attic's forced horror), an enemy (Silver Twilight Acolyte 01102's
    /// forced doom), the current act or the current agenda.
    Ability(AbilitySource),
    /// A Fast event in the controller's hand (Axis C) — *played* rather than
    /// fired. No instance until it would enter play (events never do), and not
    /// an ability source: no bullet of `glossary/Triggered_Abilities.md` names
    /// a card in hand.
    Hand,
}

impl CandidateSource {
    /// The firing in-play instance, if any — `Some` only for an
    /// [`Ability`](Self::Ability) candidate whose source names one (a card in
    /// play, a threat-area card, or the investigator card, which is a real
    /// `CardInPlay` since #448). `None` for [`Hand`](Self::Hand) (event not yet
    /// in play) and for a location / enemy / act / agenda source, none of which
    /// carries a [`CardInstanceId`]. Feeds
    /// [`EvalContext::for_controller_with_optional_source`](crate::engine::EvalContext::for_controller_with_optional_source).
    #[must_use]
    pub fn instance(self) -> Option<CardInstanceId> {
        match self {
            CandidateSource::Ability(source) => source.instance(),
            CandidateSource::Hand => None,
        }
    }

    /// The [`AbilitySource`] this candidate fires from — `None` only for
    /// [`Hand`](Self::Hand), which is a *play*, not an ability. Unlike
    /// [`instance`](Self::instance) this keeps the board sources (the act, the
    /// agenda, a location, an enemy), which is what lets a choice inside the
    /// effect anchor to the card it is printed on (#555). Feeds
    /// [`EvalContext::for_controller_with_optional_source`](crate::engine::EvalContext::for_controller_with_optional_source),
    /// which since #834 is the *only* source an eval context carries.
    #[must_use]
    pub fn ability(self) -> Option<AbilitySource> {
        match self {
            CandidateSource::Ability(source) => Some(source),
            CandidateSource::Hand => None,
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
    /// **Which ability fires / runs**, named by where it is printed rather
    /// than by where it currently sits in the card's merged ability list
    /// (#772). A candidate is minted by a scan and re-resolved when it fires,
    /// possibly several suspensions later, and a merged position is not an
    /// identity once anything grants abilities to the card — see
    /// [`AbilityAddress`].
    pub address: AbilityAddress,
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
        address: AbilityAddress,
        source: CandidateSource,
    ) -> Self {
        Self {
            code,
            controller,
            address,
            source,
        }
    }
}

// --- Window and timing frame payloads (#929) ---

/// An event reaction window or the #213 forced run, keyed by the
/// [`TimingEvent`] that opened it (EmitEvent-frame
/// Slice A, #433). The [`mode`](TimingMode) distinguishes a skippable
/// reaction window from the mandatory forced run (which carries no resume
/// continuation — on close the `drive` loop re-dispatches the exposed parent
/// frame, #434). The `TimingEvent` is referenced in place rather than
/// relocated — [`Effect`](Continuation::Effect) already holds a `crate::engine`
/// type ([`EvalContext`], #345).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingPointWindowFrame {
    /// The timing event that opened this window/run.
    pub event: TimingEvent,
    /// The timing cell whose scan produced `candidates` — the cell the
    /// `when → at → after` coordinator was resolving (#434/#702), or the
    /// caller-named cell of one of the three conditions that still bypass it.
    /// Carried so the fire-time re-validation of a reaction window (#568) can
    /// re-ask the scan the *same* question it was first asked; re-deriving it
    /// from `event` would answer for the wrong cell.
    pub bucket: EventTiming,
    /// Reaction window vs. forced run.
    pub mode: TimingMode,
    /// Candidates in resolution order (lead-ordered for the forced run;
    /// active-investigator-first for a reaction window).
    pub candidates: Vec<ResolutionCandidate>,
}

/// A framework "red-box" player window — a Rules-Reference timing step
/// that gates Fast actions and runs a per-step continuation on close
/// (EmitEvent-frame Slice A, #433). The [`FastWindowKind`] discriminant
/// routes the close continuation (`Phase` → the `*Phase`
/// anchor's `on_child_pop`; `SkillTest` → the skill-test driver). Carries no
/// `TimingEvent` — framework windows are not event-driven.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastWindowFrame {
    /// Fast-play candidates (hand Fast events admitted at this window).
    /// Usually empty (a pure Fast-gate) — non-empty only for an
    /// Axis-C hand play offered at a framework step.
    pub candidates: Vec<ResolutionCandidate>,
    /// Which investigators may submit Fast actions here.
    pub fast_actors: FastActorScope,
    /// The framework step this window gates (and its event-payload kind).
    pub kind: FastWindowKind,
}

/// A no-choice forced ability is about to resolve and the game is in
/// interactive mode (`interactive_acknowledge`): surface it as a one-option
/// pick so the player "performs" it before it lands (#466). Pushed by
/// `queue_forced_triggers` (the single-hit path) *above* the forced effect's
/// root frame; the `drive` loop suspends here, and on resume pops, letting the
/// effect frame beneath resolve. `candidate` is the forced ability's
/// [`ResolutionCandidate`] — its `code` names the prompt and its `source`
/// anchors the option to its source card (an in-play instance, or the act) (#553).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcknowledgeForcedFrame {
    /// The forced ability being acknowledged.
    pub candidate: ResolutionCandidate,
}

/// Coordinator: walk one triggering condition's timing sequence — the three
/// cells `When → At → After` with the condition's *own* resolution between
/// the first two (EmitEvent-frame C-coordinators, #434; the resolve step,
/// #701). `step` is the cursor. Pushed by `queue_event` for **every**
/// triggering condition (#702; no exceptions since #704);
/// the `drive` loop dispatches it, pushing a
/// [`TimingPoint`](TimingPointFrame) per populated cell and re-scanning
/// each cell fresh. Suspends wherever a cell opens a window — the round-end
/// `when` act-advance, a clue discovery's `when` replacement (#703).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmitEventFrame {
    /// The game event whose timing cells are being walked.
    pub event: TimingEvent,
    /// The sequence cursor (`When` → `ResolveCondition` → `At` → `After`).
    ///
    /// Renamed from `bucket` with no compatibility shim: a coordinator frame
    /// exists only in memory, mid-sequence, between two `apply` calls, and
    /// the server persists a seed state plus a `ResolveInput`-only action log
    /// (`crates/server/src/session.rs`), so no persisted artifact can carry
    /// this field.
    pub step: EmitStep,
}

/// Coordinator: one timing bucket of an [`EmitEvent`](EmitEventFrame) walk,
/// running forced then reaction (`sub` cursor). What single-bucket
/// `queue_event` does today, parameterized by bucket and made frame-resumable
/// (#434). Child of an `EmitEvent` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingPointFrame {
    /// The game event (carried for the forced/reaction scans).
    pub event: TimingEvent,
    /// Which bucket this point resolves.
    pub bucket: EventTiming,
    /// The forced-then-reaction sub-cursor.
    pub sub: TimingSub,
}

// --- Draw, encounter and play frame payloads (#931) ---

/// A skill test paused on its Mind-over-Matter "use X in place of Y?" prompt
/// at initiation (#322), migrated off the former
/// `GameState::pending_substitution_prompt` field (#348). Pushed *above* the
/// `SkillTest` frame, so top-frame dispatch routes it before the commit
/// window; resumed by [`resume_substitution_choice`](crate::engine).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubstitutionPromptFrame {
    /// The investigator taking the test.
    pub investigator: InvestigatorId,
}

/// The setup mulligan loop (Rules Reference p.27), migrated off the former
/// `GameState::mulligan_pending` cursor field (#348). `remaining[0]` is the
/// investigator currently prompted to mulligan; the queue is the Active
/// investigators in [`turn_order`](crate::state::game_state::GameState::turn_order). Pushed by
/// `start_scenario`, advanced by `resume_mulligan` as each investigator
/// submits their `PickMultiple` redraw indices, popped when drained — at
/// which point setup ends and the Investigation phase begins. While present,
/// the engine rejects every non-`ResolveInput` action. Read the prompted
/// investigator via [`GameState::current_mulligan`](crate::state::game_state::GameState::current_mulligan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MulliganFrame {
    /// Active investigators yet to mulligan, in player order; front =
    /// currently prompted.
    pub remaining: Vec<InvestigatorId>,
}

/// The Mythos step-1.4 encounter-draw loop (Rules Reference p.24), migrated
/// off the former `GameState::mythos_draw_pending` cursor field (#348).
/// `remaining[0]` is the investigator currently prompted to draw; the queue
/// is the Active investigators in [`turn_order`](crate::state::game_state::GameState::turn_order).
/// Pushed by `mythos_phase`, advanced by `resume_encounter_draw` as each
/// investigator confirms (pushing a [`PlayerDraw`](Continuation::PlayerDraw)
/// frame that owns that drawer's surge chain), popped when drained — at which
/// point the post-1.4 `MythosAfterDraws` window opens. While present, the
/// engine rejects every non-`ResolveInput` action. Read the prompted drawer
/// via [`GameState::current_encounter_drawer`](crate::state::game_state::GameState::current_encounter_drawer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterDrawFrame {
    /// Active investigators yet to draw, in player order; front =
    /// currently prompted.
    pub remaining: Vec<InvestigatorId>,
}

/// One drawer's Mythos surge chain (#423 / callsite-migration). Pushed by
/// [`EncounterDraw`](Continuation::EncounterDraw)'s `Confirm` for the current
/// drawer (above the loop frame); owns the surge cap budget across input
/// round-trips. The `drive` loop's `PlayerDraw` arm drives it: on the first
/// step (`chain_count == 0`) or when `surge_pending`, it draws the next card
/// — bumping `chain_count`, enforcing [`MAX_SURGE_CHAIN`](crate::engine), and
/// pushing an [`EncounterCard`](Continuation::EncounterCard) frame whose
/// disposal exposes this one again; otherwise it pops itself and advances the
/// loop to the next drawer. A mid-chain spawn-engagement tie pushes a
/// [`SpawnEngage`](Continuation::SpawnEngage) frame *above* this one. Never
/// awaits input itself (mirrors `EncounterCard`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerDrawFrame {
    /// Whose surge chain this is (the current `EncounterDraw` drawer).
    pub investigator: InvestigatorId,
    /// Cards drawn so far in this chain. `0` means "haven't drawn the first
    /// card yet"; bumped per draw and capped at
    /// [`MAX_SURGE_CHAIN`](crate::engine).
    pub chain_count: usize,
    /// Whether the last-drawn card carried `surge` — i.e. whether the next
    /// drive step draws another card. `false` on the first step (no card
    /// drawn yet; `chain_count == 0` triggers the first draw instead).
    pub surge_pending: bool,
}

/// A drawn encounter card whose Revelation is mid-resolution (#380), tagged
/// with how the framework disposes of it once the Revelation's whole
/// sub-resolution completes (#423). Pushed by `resolve_encounter_card`
/// *before* it runs the Revelation; sits beneath any suspension the
/// Revelation opens (a skill test, a choice, a nested effect). When that
/// sub-resolution completes and this frame is top again, the **framework**
/// disposes of the card per its [`EncounterDisposition`] and pops:
/// a treachery (`Discard`) goes to `encounter_discard` (or — if persistent —
/// is skipped, having placed itself during its Revelation); an enemy
/// (`Spawn`) is minted into play. Suspension-reason-agnostic. Never emits
/// `AwaitingInput`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterCardFrame {
    /// The drawn card's code, disposed of at teardown.
    pub card: CardCode,
    /// How the framework disposes of the card once its Revelation resolves.
    pub disposition: EncounterDisposition,
}

/// A card being played from hand, mid-resolution (Slice D #423). Pushed
/// **below** the card's pushed `OnPlay`/`OnEvent` effect; when that effect
/// pops, the drive loop's `PlayFromHand` arm runs `dispose_play_from_hand`
/// (event → discard the held card; asset → enter play, emit `EnteredPlay`).
/// Single-shot: `dispose_play_from_hand` pops the frame before emitting
/// `EnteredPlay`, so the loop opens any after-enters-play window itself.
/// Framework-internal: [Driven](FrameActivity::Driven), so it never awaits
/// input, as for `EncounterCard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayFromHandFrame {
    /// The playing investigator.
    pub investigator: InvestigatorId,
    /// The card mid-play — see [`Continuation::play_in_progress`].
    pub card: Option<CardCode>,
}

/// A slot-conflicting asset play paused for the player to choose which
/// occupying asset to discard to make room (RR p.19, #498). Pushed by
/// `slots::enter_asset_making_room` when 2+ co-controlled assets occupy a
/// slot type the new asset needs; the asset stays mid-play, riding this
/// frame until the deficit is cleared, then enters play. Resumed by
/// `slots::resume_slot_discard` via a `PickSingle(OptionId)` indexing the
/// candidate list. A [Prompt](FrameActivity::Prompt); not a phase anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotDiscardFrame {
    /// The investigator playing the asset, or taking control of it.
    pub investigator: InvestigatorId,
    /// The asset mid-entry — see [`Continuation::play_in_progress`].
    ///
    /// A whole [`CardInPlay`] rather than a bare code (#772): the frame's
    /// job is to hold the card that is in no zone (ADR 0002), and the
    /// instance serves that strictly better than the code once
    /// [`TakeControl`](card_dsl::dsl::Effect::TakeControl) can be what put it
    /// there. Lita Chantler 01117 arrives here mid-Parley carrying her
    /// accumulated damage and horror, her uses and her usage counters, and
    /// a code would drop all four.
    pub card: Option<CardInPlay>,
    /// Why the asset is entering the play area — which decides whether it
    /// is announced as *entering play* once the deficit clears.
    pub entry: AssetEntry,
}

#[cfg(test)]
mod tests;
