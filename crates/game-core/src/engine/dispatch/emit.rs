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
    /// An enemy attacks an investigator (RR p.25 step 3.3) — one triggering
    /// condition in all three cells, **coordinator-owned** (#704).
    ///
    /// The `when` cell is the cancel/replacement window (Dodge 01023); the
    /// coordinator deals the attack at its resolve step; the `at` and `after`
    /// cells see the damage and horror already landed (Silver Twilight Acolyte
    /// 01102's *"**Forced** - After Silver Twilight Acolyte attacks: Place 1
    /// doom on the current agenda."*). There is no separate would-attack
    /// condition: a card declares [`EventPattern::EnemyAttacks`] plus the trigger
    /// word it prints.
    ///
    /// [`EventPattern::EnemyAttacks`]: crate::dsl::EventPattern::EnemyAttacks
    EnemyAttacks {
        /// The attacking enemy.
        enemy: EnemyId,
        /// The investigator being attacked.
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
    ///
    /// Coordinator-owned since #721: the departure itself resolves at
    /// `resolve_left_location`, so a `when` ability sees the investigator
    /// still standing at `location`, while an `at` or `after` one sees them
    /// already at `destination`.
    LeftLocation {
        /// The investigator who left.
        investigator: InvestigatorId,
        /// The location they left.
        location: LocationId,
        /// The location they are moving to. Carried because the departure's own
        /// impact — the engaged-enemy drag and the location assignment — is
        /// resolved from this event's value at the coordinator's resolve step,
        /// where `move_primary_effect`'s locals are long out of scope.
        destination: LocationId,
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
                destination: _,
            } => Some(ForcedTriggerPoint::LeftLocation {
                investigator: *investigator,
                location: *location,
            }),
            TimingEvent::EnemyAttacks {
                enemy,
                investigator,
            } => Some(ForcedTriggerPoint::EnemyAttacks {
                enemy: *enemy,
                investigator: *investigator,
            }),
            TimingEvent::EnemyAttackDamagedSelf { .. }
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
            // The **bare milestones**: nothing about the game state changes as
            // the condition itself resolves, so there is no impact for a caller
            // to have mutated ahead of the emit and none for the coordinator to
            // perform. Each owns a no-op resolve step and each `when` cell is
            // genuinely safe to walk. In all three the teardown that *looks*
            // like the impact runs after the whole sequence, on a frame the
            // `drive` loop re-exposes once the sequence has drained:
            //
            // - `RoundEnded` — expiring "until the end of the round" effects and
            //   Upkeep → Mythos run on the Upkeep anchor's `AfterRoundEnd`
            //   resume, which is why act 01109 The Barrier's *"When the round
            //   ends"* objective has resolved in the `when` cell all along.
            // - `GameEnd` — the victory-display scan and `apply_resolution` run
            //   at the apply boundary, once the `ScenarioEnd` frame is re-exposed
            //   at `ScenarioEndStep::Finalize` (#720). The frame advances its own
            //   cursor before emitting, so the emit is in tail position and the
            //   ending finishes only after every ability it queued has drained —
            //   which is why Cover Up 01007's trauma, with its interactive
            //   acknowledge, can span `apply` calls.
            // - `EliminationGameEnd` — Rules Reference p.10 Elimination steps
            //   1–6 run on the re-exposed `Elimination` frame at
            //   `EliminationStep::RunSteps`, again after a tail-position emit.
            //   That includes the sentence that reads most like this condition's
            //   own impact, step 0's *"Then, remove those weaknesses from the
            //   game."* — the engine discharges it through step 1's threat-area
            //   partition, so it too runs after the whole sequence rather than
            //   between the cells. It migrates with `GameEnd` and for the same
            //   card: a weakness prints one *"when the game ends"* trigger, and
            //   Cover Up's `EventPattern::GameEnd` declaration is scanned by both
            //   points, so leaving this one caller-owned would reject the
            //   elimination that the retag was meant to serve.
            TimingEvent::RoundEnded
            | TimingEvent::GameEnd
            | TimingEvent::EliminationGameEnd { .. } => {
                ConditionResolution::Coordinator(resolve_bare_milestone)
            }
            // The first migration (#703). Cover Up 01007 prints *"[reaction] When
            // you would discover 1 or more clues at your location: Discard that
            // many clues from Cover Up instead"*, so the discovery has to be able
            // to not happen — which it cannot be if the caller has already moved
            // the clues by the time the `when` cell runs.
            TimingEvent::DiscoverClues { .. } => {
                ConditionResolution::Coordinator(resolve_clue_discovery)
            }
            // The second migration (#704). Dodge 01023 prints *"Fast. Play when
            // an enemy attacks an investigator at your location. / Cancel that
            // attack."*, so the attack has to be able to not happen — which it
            // cannot be if the caller has already dealt the damage by the time
            // the `when` cell runs. The attack's impact is the damage and horror
            // it places, so that is what moves to the resolve step; the
            // attacker's *exhaust* does not (`data/arkhamdb-faq/core/01023.md`:
            // *"If an attack was cancelled during the Enemy phase, the
            // attacking enemy still exhausts."*), and lives on the parked
            // `AttackLoop` frame instead.
            TimingEvent::EnemyAttacks { .. } => {
                ConditionResolution::Coordinator(resolve_enemy_attack)
            }
            // The third migration (#721). Barricade 01038 prints *"**Forced** -
            // When an investigator leaves attached location: Discard
            // Barricade."*, so the discard has to resolve before the departure
            // lands — which it cannot if the caller has already moved the
            // investigator by the time the `when` cell runs. The departure's
            // impact is the engaged-enemy drag and the location assignment, so
            // that is what moves to the resolve step; the destination *reveal*
            // and the auto-engagement do not, being the arrival's business, and
            // stay on the `MoveEnter` frame parked beneath the emit (#569).
            TimingEvent::LeftLocation { .. } => {
                ConditionResolution::Coordinator(resolve_left_location)
            }
            // Every other condition is still caller-owned: the emitting call site
            // mutates the board and *then* emits. Each flips to a coordinator-
            // owned arm when a card demands its `when` cell — what that costs,
            // and the terminal condition for the arm itself, is on
            // [`ConditionResolution::Caller`].
            TimingEvent::EnteredLocation { .. }
            | TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::EnemyDefeated { .. }
            | TimingEvent::EndOfTurn { .. }
            | TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::SkillTestResolved { .. }
            | TimingEvent::EnteredPlay { .. } => ConditionResolution::Caller,
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
    /// resume. `resolve_enemy_attack` (#704) is the dearer worked example: the
    /// mutation was tangled into a loop that read the stack after emitting, so
    /// migrating it meant parking that loop on its own frame first.
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
/// round, turn, step or game boundary whose occurrence changes nothing by
/// itself. Nothing to resolve, so nothing happens here; the cells around it are
/// the whole of the sequence.
///
/// A milestone is bare when the teardown that follows it belongs to the frame
/// that emitted it rather than to the condition: it runs after the whole
/// sequence, on a resume, not between the `when` and `at` cells. See the arm in
/// [`TimingEvent::condition_resolution`] for that argument on each of the three
/// members, and
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
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

/// The resolve step of [`TimingEvent::EnemyAttacks`]: deal the attack (#704).
///
/// The impact of "an enemy attacks an investigator" is the damage and horror it
/// places, so that is what happens here — after the `when` cell's cancel window
/// (Dodge 01023) has had its chance to prevent it, and before the `at` and
/// `after` cells see the board. May suspend on the interactive soak
/// distribution (#44/K5b); the coordinator has already advanced its cursor to
/// `At`, so the resumed prompt lands back on the `at` cell rather than dealing
/// the attack twice.
///
/// **Not** here: the attacker's exhaust. `data/arkhamdb-faq/core/01023.md`,
/// verbatim — *"If an attack was cancelled during the Enemy phase, the attacking
/// enemy still exhausts."* — so exhausting cannot be part of the impact a cancel
/// prevents. It belongs to the enemy phase's own step and lives on the parked
/// [`AttackLoop`](crate::state::Continuation::AttackLoop) frame, which the
/// `drive` loop re-exposes once this whole sequence has run. That also matches
/// `Appendix_II_Timing_and_Gameplay.md` step 3.3: *"Upon completion of dealing
/// the attack (and all abilities triggered by the attack), exhaust the enemy."*
/// — which is after the `after` cell, not before it.
fn resolve_enemy_attack(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::EnemyAttacks {
        enemy,
        investigator,
    } = event
    else {
        unreachable!("resolve_enemy_attack: not an EnemyAttacks event: {event:?}");
    };
    super::combat::deal_enemy_attack(cx, *investigator, *enemy)
}

/// The resolve step of [`TimingEvent::LeftLocation`]: the departure lands
/// (#721).
///
/// The impact of "an investigator leaves a location" is the investigator (and
/// the enemies engaged with them) arriving at the destination, so that is what
/// happens here — after the `when` cell has had its chance to interrupt, and
/// before the `at` and `after` cells see the board. Barricade 01038's
/// *"**Forced** - When an investigator leaves attached location: Discard
/// Barricade."* is the `when`-cell ability this exists for.
///
/// **Not** here: the destination reveal and the entered location's
/// auto-engagement. Both belong to the arrival, not the departure, and stay on
/// the [`MoveEnter`](crate::state::Continuation::MoveEnter) frame parked beneath
/// the emit — which is also what keeps #569's ordering, entering after leaving.
fn resolve_left_location(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let TimingEvent::LeftLocation {
        investigator,
        location,
        destination,
    } = event
    else {
        unreachable!("resolve_left_location: not a LeftLocation event: {event:?}");
    };
    super::actions::resolve_departure(cx, *investigator, *location, *destination);
    EngineOutcome::Done
}

/// Dispatch a timing event: push its `when → resolve → at → after` coordinator
/// (#702). **Every** triggering condition goes through it — since #704 there is
/// no bypass and no per-condition table of any kind here.
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
/// path — the last two bypasses, the enemy attack and its soak window, walk the
/// coordinator since #704. A cell is populated iff [the coordinator's per-cell
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
    cx.state
        .continuations
        .push(crate::state::Continuation::EmitEvent {
            event: event.clone(),
            step: crate::state::EmitStep::When,
        });
    EngineOutcome::Done
}
