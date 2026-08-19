//! The unified trigger-dispatch chokepoint (umbrella §2 / Axis-B T5a).
//!
//! [`queue_event`] is the single entry point for forced + reaction trigger
//! dispatch at a framework/game timing point. A [`TimingEvent`] names the
//! timing point and carries its binding context; `queue_event` runs the
//! two phases — Rules Reference p.2 forced-then-reaction:
//!
//! 1. **forced** — mandatory abilities are queued (via
//!    [`queue_forced_triggers`] for a lone hit, or the lead-ordered run for
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
use super::forced_triggers::{collect_forced_hits, queue_forced_triggers, ForcedTriggerPoint};
use super::Cx;

/// A game/framework timing point at which forced and/or reaction triggers
/// may fire, with the binding context the fired effects need.
///
/// The union of `ForcedTriggerPoint` (the forced dispatch key) and the
/// event-driven reaction-window points (the reaction dispatch key). Each
/// variant maps to an optional forced point (`forced_point`) and to whether
/// it opens a reaction window (`opens_reaction_window`); `EnemyDefeated` and
/// `SkillTestResolved` are **dual** (both forced and reaction at the
/// same point).
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
    /// An investigator is about to discover clues (reaction-only, Before).
    /// Opens the `BeforeDiscoverClues` replacement window — Cover Up 01007.
    /// (Axis D #336; migrated from the C5a `clue_interrupt` seam.)
    WouldDiscoverClues {
        investigator: InvestigatorId,
        location: LocationId,
        /// Clues that would actually be discovered — already capped at the
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
            | TimingEvent::WouldDiscoverClues { .. } => None,
        }
    }

    /// The RR timing bucket at which this event's reaction window opens. Read
    /// when opening a single-bucket event's window (`queue_reaction_window`) so
    /// the scan filters reactions to the right `EventTiming`. The Before-windows
    /// (`EnemyAttacks`, `WouldDiscoverClues`) and the round-end act advance are
    /// `When`; every other reaction-capable point is `After`. Forced-only events
    /// never open a reaction window, so their value here is moot (`After`).
    pub(crate) fn reaction_bucket(&self) -> crate::dsl::EventTiming {
        use crate::dsl::EventTiming;
        match self {
            TimingEvent::EnemyAttacks { .. }
            | TimingEvent::WouldDiscoverClues { .. }
            | TimingEvent::RoundEnded => EventTiming::When,
            _ => EventTiming::After,
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
            // Every other condition is still caller-owned: the emitting call site
            // mutates the board and *then* emits. Each flips to a coordinator-
            // owned arm when a card demands its `when` cell (#702–#704).
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
            | TimingEvent::WouldDiscoverClues { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::LeftLocation { .. } => ConditionResolution::Caller,
        }
    }

    /// Whether this timing event opens a reaction window. `true` only for the
    /// reaction-capable points.
    pub(crate) fn opens_reaction_window(&self) -> bool {
        matches!(
            self,
            TimingEvent::EnemyDefeated { .. }
                | TimingEvent::EnemyAttackDamagedSelf { .. }
                | TimingEvent::SkillTestResolved { .. }
                | TimingEvent::EnemyAttacks { .. }
                | TimingEvent::WouldDiscoverClues { .. }
                | TimingEvent::EnteredPlay { .. }
        )
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
    /// the resolve step. Worked examples land with #703 (clue discovery) and #704
    /// (the enemy attack), whose mutations are already factored out for a resume
    /// frame to call.
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

/// Dispatch a timing event: queue its reaction window (phase 2), then queue
/// its forced abilities (phase 1).
///
/// # This queues; it does not resolve
///
/// Every path here pushes frames for the `drive` loop to own: a lone forced hit
/// becomes an effect frame (plus an `AcknowledgeForced` above it in interactive
/// mode), 2+ simultaneous hits become a lead-ordered `TimingPointWindow`, and a
/// reaction-capable point pushes its window. So **`Done` means queued, not
/// resolved** — a caller that does synchronous work after this call pushes that
/// work *above* the abilities it just queued, and it runs *first*.
///
/// A caller with post-emit work therefore **emits in tail position and resumes
/// on its own frame**: arm the resume cursor (`enemy_phase_end` /
/// `upkeep_phase_end` re-park their phase anchor; `end_turn` arms
/// `InvestigatorTurn { ending: true }`), or push a dedicated frame beneath the
/// emit (`move_primary_effect`'s [`MoveEnter`](crate::state::Continuation::MoveEnter)),
/// then return this outcome unexamined. See
/// `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` (#569).
///
/// # Phase ordering
///
/// The reaction window is queued before the forced abilities are, so it sits
/// *beneath* them on the stack and the forced frames resolve first — RR p.2
/// forced-before-reaction, structurally rather than by hand-ordering.
///
/// Returns the forced phase's [`EngineOutcome`]: `Done` when the queueing is
/// complete, `AwaitingInput` when 2+ simultaneous forced abilities need the
/// lead's ordering pick before any of them can be queued for resolution.
#[must_use = "queue_event only queues frames: post-emit work belongs on a resume \
              frame, not after this call (ADR 0003). Return the outcome, or bind \
              it to `_` at a site whose caller owns the suspension channel"]
pub(crate) fn queue_event(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    // The only multi-bucket event (#434): cede to the `when → at → after`
    // coordinator + the global loop. `queue_event` returns `Done`; the caller
    // must do no synchronous post-emit work (`upkeep_phase_end` sets its anchor
    // resume cursor first). Every other event is single-bucket and queues
    // inline below (Checkpoint-C: no coordinator frame for a single cell).
    if matches!(event, TimingEvent::RoundEnded) {
        cx.state
            .continuations
            .push(crate::state::Continuation::EmitEvent {
                event: event.clone(),
                step: crate::state::EmitStep::When,
            });
        return EngineOutcome::Done;
    }
    if event.opens_reaction_window() {
        super::reaction_windows::queue_reaction_window(cx, event);
    }
    let Some(point) = event.forced_point() else {
        return EngineOutcome::Done;
    };
    // Forced phase. When 2+ forced abilities fire at this timing point, the lead
    // investigator orders them (#213): open the forced-resolution run and
    // suspend for the choice. 0 or 1 queue without suspending.
    let candidates = collect_forced_hits(cx.state, &point, crate::dsl::EventTiming::After);
    if candidates.len() >= 2 {
        // 2+ simultaneous forced: the lead orders them (#213). The run carries
        // no continuation (#434) — on close the `drive` loop re-dispatches the
        // exposed parent frame, which is the emit site's own resume frame (ADR
        // 0003). That is the same frame the 0/1 cases are re-exposed through, so
        // a site is correct here iff it emits in tail position — there is no
        // separate "may 2+" obligation to reason about.
        super::reaction_windows::open_forced_resolution(
            cx,
            event,
            crate::dsl::EventTiming::After,
            candidates,
        )
    } else {
        queue_forced_triggers(cx, &point, crate::dsl::EventTiming::After)
    }
}
