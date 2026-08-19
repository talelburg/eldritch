//! The unified trigger-dispatch chokepoint (umbrella §2 / Axis-B T5a).
//!
//! [`queue_event`] is the single entry point for forced + reaction trigger
//! dispatch at a framework/game timing point. A [`TimingEvent`] names the
//! timing point and carries its binding context; `queue_event` pushes the
//! [coordinator](super::coordinator) that walks the condition's
//! `when → resolve → at → after` sequence, and each cell of that walk runs the
//! two phases — Rules Reference p.2 forced-then-reaction:
//!
//! 1. **forced** — mandatory abilities are queued (via
//!    `queue_forced_triggers` for a lone hit, or the lead-ordered run for
//!    2+ simultaneous ones).
//! 2. **reaction** — the optional player reaction window opens.
//!
//! **Queues; does not resolve.** Since the effect-frame migration (#423),
//! firing an ability means *pushing a frame*: nothing here is evaluated
//! synchronously, so a returned [`EngineOutcome::Done`] means **queued**, not
//! **resolved**. A caller with work to do after the emit must put that work on
//! a resumption frame and emit in tail position; checking the returned outcome
//! is not a substitute. See
//! `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` — four call
//! sites read `Done` as "nothing happened" and ran their tails *above* the
//! abilities they had just queued (#569).
//!
//! `TimingEvent` is the merge of the engine's two pre-existing
//! binding-carrying dispatch keys: [`ForcedTriggerPoint`] (forced) and the
//! event-driven reaction-window points (reaction). T5a is a behavior-
//! preserving facade that delegates to those; it does **not** push the
//! logged [`Event`](crate::event::Event) — call sites still emit their own
//! (e.g. `EnemyDefeated`, `InvestigatorMoved`).

use crate::state::{CardCode, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase};

use serde::{Deserialize, Serialize};

use super::super::outcome::EngineOutcome;
use super::forced_triggers::ForcedTriggerPoint;
use super::Cx;

/// A game/framework timing point at which forced and/or reaction triggers
/// may fire, with the binding context the fired effects need.
///
/// The union of `ForcedTriggerPoint` (the forced dispatch key) and the
/// event-driven reaction-window points (the reaction dispatch key). Each
/// variant maps to an optional forced point (`forced_point`) and to who resolves
/// the condition itself (`condition_resolution`); `EnemyDefeated` and
/// `SkillTestResolved` are **dual** (both forced and reaction at the
/// same point). Whether a cell holds a reaction is **not** tabulated — the
/// coordinator's per-cell scan answers it (#702).
///
/// `SkillTestResolved` is the general skill-test-outcome timing point (RR
/// ST.6), of which "after you successfully investigate" (Obscuring Fog forced +
/// Dr. Milan reaction) is the `{ Investigate, Success }` narrowing. Routing the
/// forced and reaction phases through one `queue_event` keeps RR p.2
/// forced-before-reaction. Framework `PlayerWindow(PhaseStep)` windows are *not*
/// timing events — they have no `EventPattern` and stay on explicit
/// `open_fast_window` calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingEvent {
    /// An investigator entered a location (forced only).
    EnteredLocation {
        investigator: InvestigatorId,
        location: LocationId,
    },
    /// A phase ended (forced only).
    PhaseEnded { phase: Phase },
    /// An act advanced — its reverse resolves (forced only).
    ActAdvanced { code: CardCode },
    /// An agenda advanced — its reverse resolves (forced only).
    AgendaAdvanced { code: CardCode },
    /// An enemy was defeated. **Dual:** forced (act objectives keyed to the
    /// defeat) + the after-defeat reaction window (Roland 01001, Evidence!).
    EnemyDefeated {
        enemy: EnemyId,
        by: Option<InvestigatorId>,
        code: CardCode,
    },
    /// The round ended, step 4.6 (forced only).
    RoundEnded,
    /// An investigator's turn ended, step 2.2.2 (forced only).
    EndOfTurn { investigator: InvestigatorId },
    /// The game ended — a scenario resolution latched (forced only).
    GameEnd,
    /// The game has ended **for one eliminated investigator**, for the purpose
    /// of resolving weakness cards — Rules Reference p.10 Elimination step 0,
    /// quoted in full on [`Continuation::Elimination`](crate::state::Continuation::Elimination)
    /// (forced only, #638).
    ///
    /// Same card-facing pattern as [`GameEnd`](Self::GameEnd) (a weakness prints
    /// *"when the game ends"*, not two different triggers) but a different scan:
    /// one investigator, weaknesses only, and no `Status::Active` filter — the
    /// investigator being eliminated has already been flipped off `Active` when
    /// this fires.
    EliminationGameEnd {
        /// The investigator being eliminated.
        investigator: InvestigatorId,
    },
    /// An enemy attack soaked damage onto a controlled asset (reaction
    /// only — Guard Dog 01021's retaliate).
    EnemyAttackDamagedSelf {
        asset: CardInstanceId,
        enemy: EnemyId,
        controller: InvestigatorId,
    },
    /// A skill test resolved (RR ST.6). **Dual:** forced + reaction. The
    /// general timing point of which "after you successfully investigate"
    /// (Obscuring Fog 01168 forced + Dr. Milan 01033 reaction) is the
    /// `{ Investigate, Success }` narrowing. Carries no location: the forced
    /// collector derives the investigated location from the still-live
    /// in-flight `SkillTest` frame (`current_skill_test().tested_location`) —
    /// teardown is at `PostOnResolution`, well after this fires. Both phases
    /// fire at one timing point, RR p.2 forced-before-reaction.
    SkillTestResolved {
        investigator: InvestigatorId,
        kind: crate::dsl::SkillTestKind,
        outcome: crate::dsl::TestOutcome,
    },
    /// An enemy is about to attack an investigator (reaction-only, Before).
    /// Opens the `BeforeEnemyAttack` cancel window — Dodge 01023. (Axis D
    /// #336.)
    EnemyAttacks {
        enemy: EnemyId,
        investigator: InvestigatorId,
    },
    /// An investigator discovers clues (reaction only, all three cells).
    /// **Coordinator-owned** (#703): the emitting `discover_clue` only caps the
    /// count and emits, and the coordinator moves the clues at its resolve step,
    /// between the `when` and `at` cells. So a `when` ability interrupts the
    /// discovery before the clues move — Cover Up 01007's replacement — while an
    /// `at` or `after` one sees them already moved.
    DiscoverClues {
        investigator: InvestigatorId,
        location: LocationId,
        /// Clues that will actually be discovered — already capped at the
        /// location's clue count by the emitting `discover_clue`, so a
        /// replacement effect's "discard that many" (Cover Up) reads the real
        /// quantity, not the requested one (#471).
        count: u8,
    },
    /// A card entered play (reaction-only, After). Opens the `AfterEnteredPlay`
    /// window — Research Librarian 01032's tutor.
    EnteredPlay {
        /// The card instance that entered play (self-binding scope).
        instance: CardInstanceId,
        /// The investigator who controls it.
        controller: InvestigatorId,
    },
    /// An investigator left a location (forced only — Barricade 01038's
    /// self-discard). Scans the left location's attachment zone.
    LeftLocation {
        /// The investigator who left.
        investigator: InvestigatorId,
        /// The location they left.
        location: LocationId,
    },
}

impl TimingEvent {
    /// The forced dispatch point for this timing event, if it fires forced
    /// abilities. `None` for the reaction-only `EnemyAttackDamagedSelf`.
    /// `pub(super)` so the coordinator ([`super::coordinator`]) can re-scan a
    /// bucket's forced abilities (#434).
    pub(super) fn forced_point(&self) -> Option<ForcedTriggerPoint> {
        match self {
            TimingEvent::EnteredLocation {
                investigator,
                location,
            } => Some(ForcedTriggerPoint::EnteredLocation {
                investigator: *investigator,
                location: *location,
            }),
            TimingEvent::PhaseEnded { phase } => {
                Some(ForcedTriggerPoint::PhaseEnded { phase: *phase })
            }
            TimingEvent::ActAdvanced { code } => {
                Some(ForcedTriggerPoint::ActAdvanced { code: code.clone() })
            }
            TimingEvent::AgendaAdvanced { code } => {
                Some(ForcedTriggerPoint::AgendaAdvanced { code: code.clone() })
            }
            TimingEvent::EnemyDefeated { code, .. } => {
                Some(ForcedTriggerPoint::EnemyDefeated { code: code.clone() })
            }
            TimingEvent::RoundEnded => Some(ForcedTriggerPoint::RoundEnded),
            TimingEvent::EndOfTurn { investigator } => Some(ForcedTriggerPoint::EndOfTurn {
                investigator: *investigator,
            }),
            TimingEvent::GameEnd => Some(ForcedTriggerPoint::GameEnd),
            TimingEvent::EliminationGameEnd { investigator } => {
                Some(ForcedTriggerPoint::EliminationGameEnd {
                    investigator: *investigator,
                })
            }
            TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome,
            } => Some(ForcedTriggerPoint::SkillTestResolved {
                investigator: *investigator,
                kind: *kind,
                outcome: *outcome,
            }),
            TimingEvent::LeftLocation {
                investigator,
                location,
            } => Some(ForcedTriggerPoint::LeftLocation {
                investigator: *investigator,
                location: *location,
            }),
            TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::DiscoverClues { .. } => None,
        }
    }

    /// Who resolves this triggering condition's own impact — step 2 of the
    /// sequence in `glossary/Nested_Sequences.md`. An **exhaustive** match, so a
    /// new timing event cannot compile without choosing an arm; see
    /// [`ConditionResolution`] and
    /// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
    pub(crate) fn condition_resolution(&self) -> ConditionResolution {
        match self {
            // The round ending is a **bare milestone**: nothing about the game
            // state changes as the condition itself resolves (the round-end
            // teardown — expiring "until the end of the round" effects, Upkeep →
            // Mythos — runs after the whole sequence, on the Upkeep anchor's
            // `AfterRoundEnd` resume). There is no impact for a caller to have
            // mutated ahead of the emit and none for the coordinator to perform,
            // so the coordinator owns a no-op step and the `when` cell is safe to
            // walk — which is why act 01109 The Barrier's *"When the round ends"*
            // objective resolves there today.
            TimingEvent::RoundEnded => ConditionResolution::Coordinator(resolve_bare_milestone),
            // The first migration (#703). Cover Up 01007 prints *"[reaction] When
            // you would discover 1 or more clues at your location: Discard that
            // many clues from Cover Up instead"*, so the discovery has to be able
            // to not happen — which it cannot be if the caller has already moved
            // the clues by the time the `when` cell runs.
            TimingEvent::DiscoverClues { .. } => {
                ConditionResolution::Coordinator(resolve_clue_discovery)
            }
            // Every other condition is still caller-owned: the emitting call site
            // mutates the board and *then* emits. Each flips to a coordinator-
            // owned arm when a card demands its `when` cell (#704).
            TimingEvent::EnteredLocation { .. }
            | TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::EnemyDefeated { .. }
            | TimingEvent::EndOfTurn { .. }
            | TimingEvent::GameEnd
            | TimingEvent::EliminationGameEnd { .. }
            | TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::SkillTestResolved { .. }
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::LeftLocation { .. } => ConditionResolution::Caller,
        }
    }
}

/// Who resolves a triggering condition's own impact — step 2 of the sequence in
/// `glossary/Nested_Sequences.md`, which the Rules Reference puts *inside* the
/// sequence: *"1) execute "when..." effects that interrupt that triggering
/// condition, (2) resolve the triggering condition, and then, (3) execute
/// "after..." effects in response to that triggering condition."*
///
/// Returned by `TimingEvent::condition_resolution`, an exhaustive match: a new
/// timing event cannot compile without a decision — the discipline ADR 0004
/// established for classifying continuation frames. Never stored on a frame; the
/// coordinator recomputes it from the event's own value at
/// [`EmitStep::ResolveCondition`](crate::state::EmitStep::ResolveCondition),
/// because frames are serialized and cannot hold a fn pointer.
///
/// See `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConditionResolution {
    /// The **coordinator** resolves the condition, between the `when` and `at`
    /// cells. The `when` cell is walked: an interrupt there resolves before the
    /// condition's impact lands, which is what the card prints.
    Coordinator(ResolveConditionFn),
    /// The **emitting caller** mutates the board and then emits, so the
    /// condition has already resolved by the time the coordinator runs.
    ///
    /// The migration arm. Its `when` cell is *not* walked and an interrupt
    /// declared on it is rejected rather than dropped: the caller has already
    /// mutated, so resolving the interrupt here would resolve it *after* the
    /// thing it was meant to interrupt — worse than not resolving it at all.
    ///
    /// **Migrating one event** means: move the caller's mutation into a named
    /// function (the emit site keeps only the emit, in tail position per ADR
    /// 0003), point a `Coordinator` arm at it, and let the coordinator call it at
    /// the resolve step. The worked example is `resolve_clue_discovery` (#703):
    /// that mutation was already factored into `perform_discovery` for a resume
    /// frame to call, so the migration moved the call site and deleted the
    /// resume. #704 does the same for the enemy attack.
    ///
    /// **This arm exists to migrate existing callers, not to accommodate new
    /// ones.** A new timing event has no legacy emit site to invert, so it is
    /// born `Coordinator`. When the last member migrates, this arm — and the
    /// classification, and the reject — are deleted together.
    Caller,
}

/// A coordinator-owned condition's resolution step: what the condition itself
/// does to the game state, run between the `when` and `at` cells.
///
/// A plain fn pointer rather than a closure — the coordinator frame is part of
/// serialized game state, so the step is dispatched from the timing event's own
/// value on every visit instead of being captured when the frame is pushed.
pub(crate) type ResolveConditionFn = fn(&mut Cx, &TimingEvent) -> EngineOutcome;

/// The resolve step of a condition that is a **bare milestone** — a phase,
/// round, turn or step boundary whose occurrence changes nothing by itself.
/// Nothing to resolve, so nothing happens here; the cells around it are the
/// whole of the sequence.
fn resolve_bare_milestone(_cx: &mut Cx, _event: &TimingEvent) -> EngineOutcome {
    EngineOutcome::Done
}

/// The resolve step of [`TimingEvent::DiscoverClues`]: move the clues (#703).
///
/// The impact of "you discover clues" is the clues arriving on the investigator,
/// so this is where they move — after the `when` cell's replacement effects have
/// had their chance to cancel it, and before the `at` and `after` cells see the
/// board. `count` is the count fixed at the moment of the would-be discovery;
/// `perform_discovery`'s own `min` is the shrinkage backstop for a `when`
/// reaction that removed clues in between (#471).
fn resolve_clue_discovery(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::DiscoverClues {
        investigator,
        location,
        count,
    } = event
    else {
        unreachable!("resolve_clue_discovery: not a DiscoverClues event: {event:?}");
    };
    crate::engine::evaluator::perform_discovery(cx, *location, *count, *investigator);
    EngineOutcome::Done
}

/// Dispatch a timing event: push its `when → resolve → at → after` coordinator
/// (#702).
///
/// # This queues; it does not resolve
///
/// Every path here pushes frames for the `drive` loop to own. So **`Done` means
/// queued, not resolved** — a caller that does synchronous work after this call
/// pushes that work *above* the abilities it just queued, and it runs *first*.
///
/// A caller with post-emit work therefore **emits in tail position and resumes
/// on its own frame**: arm the resume cursor (`enemy_phase_end` /
/// `upkeep_phase_end` re-park their phase anchor; `end_turn` arms
/// `InvestigatorTurn { ending: true }`), or push a dedicated frame beneath the
/// emit (`move_primary_effect`'s [`MoveEnter`](crate::state::Continuation::MoveEnter)),
/// then return this outcome unexamined. See
/// `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` (#569).
///
/// # The sequence
///
/// `glossary/Nested_Sequences.md` gives *every* triggering condition the same
/// three-part sequence, and `glossary/At.md` adds the middle cell — so there is
/// no per-event table of which cells an event supports and no single-cell fast
/// path. A cell is populated iff [the coordinator's per-cell
/// scan](super::coordinator::dispatch_emit_event) finds something in it, and an
/// empty cell is skipped without prompting. Forced-before-reaction within a cell
/// (RR p.2) is the [`TimingPoint`](crate::state::Continuation::TimingPoint)
/// frame's `sub` cursor.
///
/// Returns `Done`: the walk itself never suspends here, because pushing the
/// coordinator *is* the whole of the queueing. The suspensions it will produce —
/// a reaction window, or the lead's ordering pick for 2+ simultaneous forced
/// abilities (#213) — surface from the `drive` loop as it walks the cells, which
/// is why a call site's only obligation is to emit in tail position.
#[must_use = "queue_event only queues frames: post-emit work belongs on a resume \
              frame, not after this call (ADR 0003). Return the outcome, or bind \
              it to `_` at a site whose caller owns the suspension channel"]
pub(crate) fn queue_event(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    // The enemy-attack machinery's two remaining conditions keep their pre-#702
    // single-cell handling; every other condition walks the coordinator.
    //
    // Both share one obstruction: their emit sites read the stack *synchronously*
    // after the emit — `open_windows()` at
    // `combat::deal_head_and_maybe_park` / `process_head_attacker` — which is
    // the shape ADR 0003 forbids, and which a coordinator frame breaks (the
    // window it will open is not open yet when the caller looks). Clearing it
    // means the attack loop stops draining inline and parks on its `AttackLoop`
    // frame for the `drive` loop to re-expose, so the migration is its own
    // ticket (#704) rather than a line here.
    //
    // - `EnemyAttacks` (Dodge 01023's cancel) is additionally
    //   **interrupt-timed**: caller-owned with a populated `when` cell, so
    //   walking it would hit the caller-owned `when`-cell reject and take Dodge
    //   down with it.
    // - `EnemyAttackDamagedSelf` (Guard Dog 01021's soak retaliate) is
    //   modelled `after`-timed, so under today's declaration nothing about
    //   *which cell* it resolves in is at stake — only the drive shape is. It
    //   rides along with #704, which owns the same loop.
    //
    //   The declaration is itself wrong, which is why this holdout is not as
    //   cheap as it looks. Guard Dog prints *"[reaction] **When** an enemy
    //   attack deals damage to Guard Dog: Deal 1 damage to the attacking
    //   enemy."* — one of the mis-tagged declarations #694 retags — so once it
    //   is `when`-timed it needs a walked `when` cell, and this condition must
    //   be coordinator-owned before that can happen.
    //
    // `DiscoverClues` was the third until #703 migrated it: its mutation was
    // already factored into `perform_discovery`, so the emit site's stack peek
    // could just be deleted rather than a loop restructured.
    if let Some(bucket) = single_cell_holdout_bucket(event) {
        super::reaction_windows::queue_reaction_window(cx, event, bucket);
        return EngineOutcome::Done;
    }
    cx.state
        .continuations
        .push(crate::state::Continuation::EmitEvent {
            event: event.clone(),
            step: crate::state::EmitStep::When,
        });
    EngineOutcome::Done
}

/// `Some(cell)` for a [single-cell holdout](queue_event) — the one cell it opens
/// its reaction window at — and `None` for every condition that walks the
/// coordinator. Membership and cell are one answer rather than two, so the set
/// cannot be widened in one place and forgotten in the other.
///
/// The last remnant of the per-condition cell table #702 deleted, kept alive only
/// by the two conditions that still bypass the coordinator. It goes with them
/// (#704).
fn single_cell_holdout_bucket(event: &TimingEvent) -> Option<crate::dsl::EventTiming> {
    use crate::dsl::EventTiming;
    match event {
        // Interrupt timing: Dodge 01023 cancels the attack before its impact
        // lands.
        TimingEvent::EnemyAttacks { .. } => Some(EventTiming::When),
        // Guard Dog 01021 is *declared* after-timed, though it prints "when" —
        // see the note on `queue_event`.
        TimingEvent::EnemyAttackDamagedSelf { .. } => Some(EventTiming::After),
        _ => None,
    }
}
