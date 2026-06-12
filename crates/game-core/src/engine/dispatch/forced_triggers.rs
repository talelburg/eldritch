//! Forced-trigger dispatch: fires `Trigger::OnEvent` abilities printed
//! on scenario-structure cards (locations, acts, agendas) at framework
//! timing points, via an immediate path separate from the player
//! reaction-window machinery. Single-trigger only in this slice; 2+
//! simultaneous pending triggers reject loudly (#213 adds the ordering
//! loop, #212 the universal `emit_event` chokepoint).

use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger};
use crate::state::{CardCode, InvestigatorId, LocationId, Phase};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::EngineOutcome;
use super::Cx;

/// A framework timing point at which Forced (`Trigger::OnEvent`)
/// abilities on scenario-structure cards may fire. Each variant carries
/// the binding context the fired effect needs.
///
/// `pub(crate)` — not part of the public API. [`crate::test_support`]
/// constructs it internally via `fire_forced_on_enter` (a primitive-arg
/// helper), so integration tests never need to name this type directly.
/// Wired into `move_action` (`EnteredLocation`) and
/// `enemy_phase_end`/`upkeep_phase_end` (`PhaseEnded`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForcedTriggerPoint {
    /// An investigator entered a location. Scans that location's card
    /// for `EventPattern::EnteredLocation` forced abilities; binds
    /// controller = the entering investigator.
    EnteredLocation {
        /// The investigator who entered the location.
        investigator: InvestigatorId,
        /// The location that was entered.
        location: LocationId,
    },
    /// A phase ended. Scans the current act and agenda for
    /// `EventPattern::PhaseEnded { phase }` forced abilities; binds
    /// controller = the lead investigator (board-wide effects ignore it).
    PhaseEnded { phase: Phase },
    /// An act advanced (its reverse side resolves). Scans the *leaving*
    /// act's card for `EventPattern::ActAdvanced` forced abilities; binds
    /// controller = the lead investigator.
    ActAdvanced {
        /// Printed code of the act that advanced.
        code: CardCode,
    },
    /// An enemy was defeated. Scans the *current act* for
    /// `EventPattern::EnemyDefeated` forced abilities whose `code` narrow
    /// matches (or is `None`); binds controller = the lead investigator.
    /// The act-3 objective (01110) advances on the Ghoul Priest's defeat
    /// through this point.
    EnemyDefeated {
        /// Printed code of the defeated enemy (for `code`-narrow matching).
        code: CardCode,
    },
}

struct ForcedHit {
    code: CardCode,
    ability_index: usize,
    controller: InvestigatorId,
}

/// Fire Forced abilities matching `point`. Single-trigger path: 0 → Done;
/// 1 → resolve via `apply_effect`; 2+ → reject loudly (no silently-chosen
/// order — #213 adds the ordering loop).
pub(crate) fn fire_forced_triggers(cx: &mut Cx, point: &ForcedTriggerPoint) -> EngineOutcome {
    let hits = collect_forced_hits(cx.state, point);
    match hits.len() {
        0 => EngineOutcome::Done,
        1 => resolve_one(cx, &hits[0]),
        n => EngineOutcome::Rejected {
            reason: format!(
                "fire_forced_triggers: {n} simultaneous forced triggers at {point:?}; \
                 ordering not yet implemented (see #213). Slice-1 content never produces \
                 this — investigate the source."
            )
            .into(),
        },
    }
}

fn collect_forced_hits(
    state: &crate::state::GameState,
    point: &ForcedTriggerPoint,
) -> Vec<ForcedHit> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    match point {
        ForcedTriggerPoint::EnteredLocation {
            investigator,
            location,
        } => {
            let Some(loc) = state.locations.get(location) else {
                return hits;
            };
            push_matching(reg, &loc.code, *investigator, &mut hits, |p| {
                matches!(p, EventPattern::EnteredLocation)
            });
        }
        ForcedTriggerPoint::PhaseEnded { phase } => {
            let want_phase = dsl_phase(*phase);
            // Lead investigator binds the controller for board-wide effects
            // (which ignore it). First of turn_order is the lead.
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(
                    reg,
                    &act.code,
                    lead,
                    &mut hits,
                    |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
                );
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(
                    reg,
                    &agenda.code,
                    lead,
                    &mut hits,
                    |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
                );
            }
        }
        ForcedTriggerPoint::ActAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, code, lead, &mut hits, |p| {
                matches!(p, EventPattern::ActAdvanced)
            });
        }
        ForcedTriggerPoint::EnemyDefeated { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(reg, &act.code, lead, &mut hits, |p| {
                    matches!(
                        p,
                        EventPattern::EnemyDefeated { code: narrow, .. }
                            if narrow.as_deref().is_none_or(|c| c == code.as_str())
                    )
                });
            }
        }
    }
    hits
}

/// Map the engine's `state::Phase` to the `card-dsl` mirror so a
/// `PhaseEnded` pattern can be compared.
fn dsl_phase(phase: Phase) -> crate::dsl::Phase {
    match phase {
        Phase::Mythos => crate::dsl::Phase::Mythos,
        Phase::Investigation => crate::dsl::Phase::Investigation,
        Phase::Enemy => crate::dsl::Phase::Enemy,
        Phase::Upkeep => crate::dsl::Phase::Upkeep,
    }
}

fn push_matching(
    reg: &card_registry::CardRegistry,
    code: &CardCode,
    controller: InvestigatorId,
    out: &mut Vec<ForcedHit>,
    want: impl Fn(&EventPattern) -> bool,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let Trigger::OnEvent { pattern, timing } = &ability.trigger {
            // Only `After` timing is handled in this slice; no in-scope
            // Forced card uses `Before` ("when X would Y") timing.
            // Revisit when such a card lands.
            if *timing == EventTiming::After && want(pattern) {
                out.push(ForcedHit {
                    code: code.clone(),
                    ability_index: idx,
                    controller,
                });
            }
        }
    }
}

fn resolve_one(cx: &mut Cx, hit: &ForcedHit) -> EngineOutcome {
    let Some(reg) = card_registry::current() else {
        return EngineOutcome::Rejected {
            reason: "fire_forced_triggers: registry vanished between collect and resolve".into(),
        };
    };
    let Some(abilities) = (reg.abilities_for)(&hit.code) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "fire_forced_triggers: {} has no abilities at resolve time",
                hit.code
            )
            .into(),
        };
    };
    let effect = abilities[hit.ability_index].effect.clone();
    apply_effect(cx, &effect, EvalContext::for_controller(hit.controller))
}
