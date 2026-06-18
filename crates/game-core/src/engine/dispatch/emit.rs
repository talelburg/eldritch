//! The unified trigger-dispatch chokepoint (umbrella §2 / Axis-B T5a).
//!
//! [`emit_event`] is the single entry point for forced + reaction trigger
//! dispatch at a framework/game timing point. A [`TimingEvent`] names the
//! timing point and carries its binding context; `emit_event` runs the
//! two phases — Rules Reference p.2 forced-then-reaction:
//!
//! 1. **forced** — mandatory abilities resolve (today via the existing
//!    `fire_forced_triggers`; T5b replaces this with the iterative
//!    lead-investigator ordering loop).
//! 2. **reaction** — the optional player reaction window opens.
//!
//! `TimingEvent` is the merge of the engine's two pre-existing
//! binding-carrying dispatch keys: [`ForcedTriggerPoint`] (forced) and the
//! event-driven [`WindowKind`] variants (reaction). T5a is a behavior-
//! preserving facade that delegates to those; it does **not** push the
//! logged [`Event`](crate::event::Event) — call sites still emit their own
//! (e.g. `EnemyDefeated`, `InvestigatorMoved`).

use crate::state::{
    CardCode, CardInstanceId, EnemyId, ForcedContinuation, InvestigatorId, LocationId, Phase,
    WindowKind,
};

use super::super::outcome::EngineOutcome;
use super::forced_triggers::{collect_forced_hits, fire_forced_triggers, ForcedTriggerPoint};
use super::Cx;

/// A game/framework timing point at which forced and/or reaction triggers
/// may fire, with the binding context the fired effects need.
///
/// The union of [`ForcedTriggerPoint`] (the forced dispatch key) and the
/// event-driven [`WindowKind`] variants (the reaction dispatch key). Each
/// variant maps to an optional forced point ([`Self::forced_point`]) and an
/// optional reaction window ([`Self::reaction_window`]); `EnemyDefeated` and
/// `SuccessfullyInvestigated` are **dual** (both forced and reaction at the
/// same point).
///
/// `SuccessfullyInvestigated` collapses the successful-investigate moment
/// into one timing point (T5b / #213): pre-T5b its forced (Obscuring Fog)
/// fired a skill-test driver step *after* its reaction window (Dr. Milan)
/// opened — reaction-before-forced, against RR p.2. Routing both through one
/// `emit_event` restores forced-before-reaction. Framework
/// `PlayerWindow(PhaseStep)` windows are *not* timing events — they have no
/// `EventPattern` and stay on explicit `open_fast_window` calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TimingEvent {
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
    /// An enemy attack soaked damage onto a controlled asset (reaction
    /// only — Guard Dog 01021's retaliate).
    EnemyAttackDamagedSelf {
        asset: CardInstanceId,
        enemy: EnemyId,
        controller: InvestigatorId,
    },
    /// A location was successfully investigated. **Dual:** forced (Obscuring
    /// Fog 01168 discards) + the after-investigate reaction window (Dr. Milan
    /// 01033). Both fire at one timing point — RR p.2 forced-before-reaction
    /// — via this single emit, replacing the pre-T5b split where the forced
    /// fired a step *after* the reaction window opened.
    SuccessfullyInvestigated {
        investigator: InvestigatorId,
        location: LocationId,
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
}

impl TimingEvent {
    /// The forced dispatch point for this timing event, if it fires forced
    /// abilities. `None` for the reaction-only `EnemyAttackDamagedSelf`.
    fn forced_point(&self) -> Option<ForcedTriggerPoint> {
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
            TimingEvent::SuccessfullyInvestigated {
                investigator,
                location,
            } => Some(ForcedTriggerPoint::AfterLocationInvestigated {
                investigator: *investigator,
                location: *location,
            }),
            TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::EnemyAttacks { .. }
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
        }
    }

    /// The reaction window this timing event opens, if any. `Some` only for
    /// the reaction-capable points.
    fn reaction_window(&self) -> Option<WindowKind> {
        match self {
            TimingEvent::EnemyDefeated { enemy, by, .. } => Some(WindowKind::AfterEnemyDefeated {
                enemy: *enemy,
                by: *by,
            }),
            TimingEvent::EnemyAttackDamagedSelf {
                asset,
                enemy,
                controller,
            } => Some(WindowKind::AfterEnemyAttackDamagedAsset {
                asset: *asset,
                enemy: *enemy,
                controller: *controller,
            }),
            TimingEvent::SuccessfullyInvestigated { investigator, .. } => {
                Some(WindowKind::AfterSuccessfulInvestigate {
                    investigator: *investigator,
                })
            }
            TimingEvent::EnemyAttacks {
                enemy,
                investigator,
            } => Some(WindowKind::BeforeEnemyAttack {
                enemy: *enemy,
                investigator: *investigator,
            }),
            TimingEvent::WouldDiscoverClues {
                investigator,
                location,
                count,
            } => Some(WindowKind::BeforeDiscoverClues {
                investigator: *investigator,
                location: *location,
                count: *count,
            }),
            TimingEvent::EnteredPlay {
                instance,
                controller,
            } => Some(WindowKind::AfterEnteredPlay {
                instance: *instance,
                controller: *controller,
            }),
            _ => None,
        }
    }

    /// How a *forced run* opened at this timing point resumes the framework
    /// flow on close (#213). Read only when 2+ simultaneous forced abilities
    /// fire and the lead must order them — see [`emit_event`].
    ///
    /// - `Some(ForcedContinuation::Terminal)` — the emit site is genuinely
    ///   terminal: nothing in the framework runs after the forced abilities,
    ///   so the run closes to `Done`.
    /// - `Some(ForcedContinuation::…)` — the site has framework work after
    ///   the emit; the named variant resumes exactly that tail.
    /// - `None` — the site *has* a tail but its resume continuation is **not
    ///   wired**. Safe today because no such site can produce 2+ forced in
    ///   the current card pool; `emit_event` turns a 2+ hit here into a loud
    ///   `unreachable!` rather than silently dropping the tail.
    ///
    /// The match is exhaustive over [`TimingEvent`] (and over [`Phase`] for
    /// `PhaseEnded`) so adding a variant forces a deliberate decision here.
    fn forced_continuation(&self) -> Option<ForcedContinuation> {
        match self {
            // A move completes once "when you enter" forced abilities
            // resolve — nothing in the framework follows.
            TimingEvent::EnteredLocation { .. } => Some(ForcedContinuation::Terminal),
            // "Upkeep phase ends. Round ends." (RR p.24): after the round-end
            // forced abilities resolve, the upkeep step opens the act
            // round-end advance window and steps the phase.
            TimingEvent::RoundEnded => Some(ForcedContinuation::UpkeepAfterRoundEnded),
            // End of turn (RR p.24 2.2.2): after the turn-ending investigator's
            // forced abilities resolve, rotate to the next active investigator
            // or end the Investigation phase.
            TimingEvent::EndOfTurn { investigator } => {
                Some(ForcedContinuation::EndOfTurnAfterForced {
                    investigator: *investigator,
                })
            }
            // Non-terminal sites with no wired resume continuation. None can
            // produce 2+ forced in the current card pool; if one ever does,
            // emit_event's 2+ branch fires its loud guard rather than
            // dropping the tail. Add a ForcedContinuation variant + arm then.
            //
            // Extra care for the **dual** sites (`EnemyDefeated`,
            // `SuccessfullyInvestigated`): emit_event queues their reaction
            // window *before* pushing the forced run on top, so wiring a
            // continuation here must also re-surface that queued window after
            // the forced run closes — otherwise it is left stranded below the
            // forced frame (the apply loop has no post-dispatch window sweep).
            TimingEvent::PhaseEnded { .. }
            | TimingEvent::ActAdvanced { .. }
            | TimingEvent::AgendaAdvanced { .. }
            | TimingEvent::EnemyDefeated { .. }
            | TimingEvent::GameEnd
            | TimingEvent::EnemyAttackDamagedSelf { .. }
            | TimingEvent::SuccessfullyInvestigated { .. }
            // Reaction-only Before-timing points: no forced phase (Axis D).
            | TimingEvent::EnemyAttacks { .. }
            // Reaction-only After point: no forced phase (Research Librarian).
            | TimingEvent::EnteredPlay { .. }
            | TimingEvent::WouldDiscoverClues { .. } => None,
        }
    }
}

/// Dispatch a timing event: queue its reaction window (phase 2), then fire
/// its forced abilities (phase 1).
///
/// # Phase ordering
///
/// The reaction window is *queued* before forced abilities *resolve*, so
/// `WindowOpened` is emitted before the forced effects' events — preserving
/// the pre-T5a per-site order (each dual site called `queue_reaction_window`
/// before `fire_forced_triggers`). The forced abilities still **resolve**
/// synchronously here, before the player can act on the queued window (the
/// window only suspends the surrounding driver at its next step boundary),
/// so resolution is RR-correct forced-then-reaction.
///
/// Returns the forced phase's [`EngineOutcome`] (the queue itself never
/// suspends or rejects). T5b replaces the forced phase's internals with the
/// iterative ordering loop.
pub(crate) fn emit_event(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    if let Some(kind) = event.reaction_window() {
        super::reaction_windows::queue_reaction_window(cx, kind);
    }
    let Some(point) = event.forced_point() else {
        return EngineOutcome::Done;
    };
    // Forced phase. When 2+ forced abilities resolve at this timing point,
    // the lead investigator orders them (#213): open the forced-resolution
    // run and suspend for the choice. 0 or 1 resolve synchronously, as before.
    let candidates = collect_forced_hits(cx.state, &point);
    if candidates.len() >= 2 {
        // 2+ simultaneous forced: the lead orders them (#213). Resume the
        // framework flow this site suspended via its forced continuation; a
        // non-terminal site with no wired continuation is a loud bug, not a
        // silent dropped tail.
        let continuation = event.forced_continuation().unwrap_or_else(|| {
            unreachable!(
                "emit_event: 2+ simultaneous forced abilities at {event:?}, but its \
                 resume continuation isn't wired (#213) — add a ForcedContinuation \
                 variant + a TimingEvent::forced_continuation arm",
            )
        });
        super::reaction_windows::open_forced_resolution(cx, candidates, continuation)
    } else {
        fire_forced_triggers(cx, &point)
    }
}
