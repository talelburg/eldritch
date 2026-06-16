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
    CardCode, CardInstanceId, EnemyId, InvestigatorId, LocationId, Phase, WindowKind,
};

use super::super::outcome::EngineOutcome;
use super::forced_triggers::{
    collect_forced_hits, fire_forced_triggers, ForcedHit, ForcedTriggerPoint,
};
use super::Cx;

/// A game/framework timing point at which forced and/or reaction triggers
/// may fire, with the binding context the fired effects need.
///
/// The union of [`ForcedTriggerPoint`] (the forced dispatch key) and the
/// event-driven [`WindowKind`] variants (the reaction dispatch key). Each
/// variant maps to an optional forced point ([`Self::forced_point`]) and an
/// optional reaction window ([`Self::reaction_window`]); `EnemyDefeated` is
/// **dual** (both forced and reaction at the same point).
///
/// The successful-investigate moment is *not* a `TimingEvent` in T5a: its
/// forced (Obscuring Fog) and reaction (Dr. Milan) fire at different
/// skill-test driver steps today, so collapsing them into one timing event
/// would reorder them (RR forced-first) — a behavior change deferred to T5b.
/// Framework `PlayerWindow(PhaseStep)` windows are *not* timing events —
/// they have no `EventPattern` and stay on explicit `open_fast_window`
/// calls.
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
            TimingEvent::EnemyAttackDamagedSelf { .. } => None,
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
            _ => None,
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
    let hits = collect_forced_hits(cx.state, &point);
    if hits.len() >= 2 {
        let candidates = hits.into_iter().map(ForcedHit::into_candidate).collect();
        super::reaction_windows::open_forced_resolution(cx, candidates)
    } else {
        fire_forced_triggers(cx, &point)
    }
}
