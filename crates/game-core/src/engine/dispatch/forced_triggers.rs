//! Forced-trigger dispatch: fires `Trigger::OnEvent` abilities printed
//! on scenario-structure cards (locations, acts, agendas) at framework
//! timing points, via an immediate path separate from the player
//! reaction-window machinery. Multiple simultaneous triggers resolve in
//! a fixed deterministic order (see [`fire_forced_triggers`]); #213 adds
//! player-chosen ordering, #212 the universal `emit_event` chokepoint.

use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger};
use crate::state::{
    CandidateSource, CardCode, CardInstanceId, InvestigatorId, LocationId, Phase,
    ResolutionCandidate,
};

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
    /// An agenda advanced (its reverse side resolves on doom). Scans the
    /// *leaving* agenda's card for `EventPattern::AgendaAdvanced` forced
    /// abilities; binds controller = the lead investigator. The mirror of
    /// [`ActAdvanced`](Self::ActAdvanced) — fired from `advance_agenda`.
    AgendaAdvanced {
        /// Printed code of the agenda that advanced.
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
    /// The round ended (step 4.6). Scans the current act and agenda for
    /// `EventPattern::RoundEnded` forced abilities; binds controller =
    /// the lead investigator (board-wide effects ignore it).
    RoundEnded,
    /// An investigator's turn ended (step 2.2.2). Scans that
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::EndOfTurn` forced abilities; binds controller
    /// = that investigator. First consumer: Frozen in Fear (01164), C4c.
    EndOfTurn {
        /// The investigator whose turn ended.
        investigator: InvestigatorId,
    },
    /// A location was successfully investigated. Scans the investigating
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::AfterLocationInvestigated` forced abilities;
    /// binds controller = that investigator. C4c (#235) extends the scan
    /// to the investigated location's attachments for Obscuring Fog
    /// (01168), the first real consumer.
    AfterLocationInvestigated {
        /// The investigator who investigated.
        investigator: InvestigatorId,
        /// The location that was investigated. Unused by the C4a scan
        /// (which keys off the investigator); C4c reads it to scan the
        /// location's attachment zone.
        location: LocationId,
    },
    /// The game ended (a scenario resolution latched). Scans every
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::GameEnd` forced abilities; binds controller =
    /// each instance's controller. First consumer: Cover Up 01007's
    /// game-end mental-trauma forced (C5a #236).
    GameEnd,
}

/// Fire Forced abilities matching `point`, resolving each hit in a fixed
/// deterministic order.
///
/// The order is the collection order of [`collect_forced_hits`]: board
/// cards (act before agenda) before threat-area / attachment instances,
/// investigators by id (`BTreeMap`), instances in zone order. #213 will
/// replace this with player-chosen ordering (Rules Reference p.17: the
/// player orders simultaneous triggers, even in solo); a fixed order is a
/// rules-acceptable stand-in until then.
///
/// **Suspension caveat (#212 reentrancy).** A hit that suspends
/// (`AwaitingInput`) or rejects is surfaced immediately, abandoning any
/// later hits — re-entry mid-sequence isn't modeled yet. This is correct
/// as long as no point produces 2+ simultaneous *suspending* hits;
/// synchronous multi-hit points (`RoundEnded`: agenda 01107 doom +
/// Dissonant Voices 01165 discard) all resolve fully. The only suspending
/// forced effect today is Frozen in Fear 01164's `EndOfTurn` skill test;
/// since it carries no "Limit 1", two copies on one investigator would
/// drop the second copy's test at end of turn — a known #212/#213
/// limitation, not a single-hit guarantee.
pub(crate) fn fire_forced_triggers(cx: &mut Cx, point: &ForcedTriggerPoint) -> EngineOutcome {
    let hits = collect_forced_hits(cx.state, point);
    for hit in &hits {
        match resolve_one(cx, hit) {
            EngineOutcome::Done => {}
            other => return other,
        }
    }
    EngineOutcome::Done
}

// dispatcher: one match arm per ForcedTriggerPoint.
#[allow(clippy::too_many_lines)]
pub(super) fn collect_forced_hits(
    state: &crate::state::GameState,
    point: &ForcedTriggerPoint,
) -> Vec<ResolutionCandidate> {
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
            push_matching(reg, &loc.code, *investigator, None, &mut hits, |p| {
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
                    None,
                    &mut hits,
                    |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
                );
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(
                    reg,
                    &agenda.code,
                    lead,
                    None,
                    &mut hits,
                    |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
                );
            }
        }
        ForcedTriggerPoint::ActAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, code, lead, None, &mut hits, |p| {
                matches!(p, EventPattern::ActAdvanced)
            });
        }
        ForcedTriggerPoint::AgendaAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, code, lead, None, &mut hits, |p| {
                matches!(p, EventPattern::AgendaAdvanced)
            });
        }
        ForcedTriggerPoint::EnemyDefeated { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(reg, &act.code, lead, None, &mut hits, |p| {
                    matches!(
                        p,
                        EventPattern::EnemyDefeated { code: narrow, .. }
                            if narrow.as_deref().is_none_or(|c| c == code.as_str())
                    )
                });
            }
        }
        ForcedTriggerPoint::RoundEnded => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(reg, &act.code, lead, None, &mut hits, |p| {
                    matches!(p, EventPattern::RoundEnded)
                });
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(reg, &agenda.code, lead, None, &mut hits, |p| {
                    matches!(p, EventPattern::RoundEnded)
                });
            }
            // Persistent threat-area treacheries discard on RoundEnded
            // (Dissonant Voices 01165). Scan every investigator's
            // controlled instances; bind source = the instance so
            // `Effect::DiscardSelf` finds itself.
            for (inv_id, inv) in &state.investigators {
                for card in inv.controlled_card_instances() {
                    push_matching(
                        reg,
                        &card.code,
                        *inv_id,
                        Some(card.instance_id),
                        &mut hits,
                        |p| matches!(p, EventPattern::RoundEnded),
                    );
                }
            }
        }
        ForcedTriggerPoint::EndOfTurn { investigator } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Scan the ending investigator's controlled instances
            // (threat area + in play). Code-based registry lookup is
            // fine — abilities are static per code; C4c threads the
            // source instance when an effect needs to discard itself.
            for card in inv.controlled_card_instances() {
                push_matching(
                    reg,
                    &card.code,
                    *investigator,
                    Some(card.instance_id),
                    &mut hits,
                    |p| matches!(p, EventPattern::EndOfTurn),
                );
            }
        }
        ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location,
        } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Scan the investigator's controlled instances (C4a) and the
            // investigated location's attachment zone (C4c — Obscuring Fog
            // 01168 attaches to the location, not the threat area). Bind
            // source = the firing instance so `Effect::DiscardSelf` finds
            // itself.
            for card in inv.controlled_card_instances() {
                push_matching(
                    reg,
                    &card.code,
                    *investigator,
                    Some(card.instance_id),
                    &mut hits,
                    |p| matches!(p, EventPattern::AfterLocationInvestigated),
                );
            }
            if let Some(loc) = state.locations.get(location) {
                for att in &loc.attachments {
                    push_matching(
                        reg,
                        &att.code,
                        *investigator,
                        Some(att.instance_id),
                        &mut hits,
                        |p| matches!(p, EventPattern::AfterLocationInvestigated),
                    );
                }
            }
        }
        ForcedTriggerPoint::GameEnd => {
            // Scan every investigator's controlled instances; bind
            // controller = each card's controller, source = the instance.
            // `state.investigators` is a BTreeMap, so iteration order is
            // deterministic — consistent with the fixed-order contract.
            for (inv_id, inv) in &state.investigators {
                for card in inv.controlled_card_instances() {
                    push_matching(
                        reg,
                        &card.code,
                        *inv_id,
                        Some(card.instance_id),
                        &mut hits,
                        |p| matches!(p, EventPattern::GameEnd),
                    );
                }
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
    source: Option<CardInstanceId>,
    out: &mut Vec<ResolutionCandidate>,
    want: impl Fn(&EventPattern) -> bool,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let Trigger::OnEvent {
            pattern, timing, ..
        } = &ability.trigger
        {
            // Only `After` timing is handled in this slice; no in-scope
            // Forced card uses `Before` ("when X would Y") timing.
            // Revisit when such a card lands.
            if *timing == EventTiming::After && want(pattern) {
                out.push(ResolutionCandidate {
                    code: code.clone(),
                    controller,
                    ability_index: u8::try_from(idx)
                        .expect("ability_index fits u8 — abilities vecs are tiny"),
                    // Forced hits are in-play instances or scenario board
                    // cards — never hand events.
                    source: match source {
                        Some(id) => CandidateSource::InPlay(id),
                        None => CandidateSource::Board,
                    },
                });
            }
        }
    }
}

fn resolve_one(cx: &mut Cx, hit: &ResolutionCandidate) -> EngineOutcome {
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
    let effect = abilities[usize::from(hit.ability_index)].effect.clone();
    let ctx = match hit.source {
        CandidateSource::InPlay(src) => {
            EvalContext::for_controller_with_source(hit.controller, src)
        }
        CandidateSource::Board => EvalContext::for_controller(hit.controller),
        CandidateSource::Hand => unreachable!(
            "fire_forced_triggers: a forced run never holds a Hand candidate \
             (hand Fast events are reaction-window plays, not forced)"
        ),
    };
    apply_effect(cx, &effect, ctx)
}
