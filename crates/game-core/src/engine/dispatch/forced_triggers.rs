//! Forced-trigger dispatch: fires `Trigger::OnEvent` abilities printed
//! on scenario-structure cards (locations, acts, agendas) at framework
//! timing points, via an immediate path separate from the player
//! reaction-window machinery. Multiple simultaneous triggers resolve in
//! a fixed deterministic order (see [`fire_forced_triggers`]); #213 adds
//! player-chosen ordering, #212 the universal `emit_event` chokepoint.

use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger, TriggerKind};
use crate::state::{
    CandidateSource, CardCode, CardInstanceId, InvestigatorId, LocationId, Phase,
    ResolutionCandidate,
};

use super::super::evaluator::{push_effect, EvalContext};
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
    /// A skill test resolved (RR ST.6). Forced side of
    /// [`TimingEvent::SkillTestResolved`](super::emit::TimingEvent::SkillTestResolved).
    /// Scans the resolving investigator's controlled card instances (threat
    /// area + in play) **and** the investigated location's attachment zone
    /// (Obscuring Fog 01168) for matching `EventPattern::SkillTestResolved`
    /// forced abilities; binds controller = that investigator. The location is
    /// derived from the in-flight `SkillTest` frame's `tested_location` at scan
    /// time, so this point carries no location of its own.
    SkillTestResolved {
        /// The investigator who took the test.
        investigator: InvestigatorId,
        /// The test kind — matched against a listener's `kind` narrowing.
        kind: crate::dsl::SkillTestKind,
        /// The test outcome — matched against a listener's `outcome`.
        outcome: crate::dsl::TestOutcome,
    },
    /// The game ended (a scenario resolution latched). Scans every
    /// investigator's controlled card instances (threat area + in play)
    /// for `EventPattern::GameEnd` forced abilities; binds controller =
    /// each instance's controller. First consumer: Cover Up 01007's
    /// game-end mental-trauma forced (C5a #236).
    GameEnd,
    /// An investigator left a location. Scans that location's attachment zone
    /// for `EventPattern::LeftLocation` forced abilities (Barricade 01038's
    /// self-discard); binds controller = the leaving investigator, source =
    /// the firing attachment instance. Mirrors the attachment scan in
    /// [`SkillTestResolved`](Self::SkillTestResolved).
    LeftLocation {
        /// The investigator who left.
        investigator: InvestigatorId,
        /// The location they left.
        location: LocationId,
    },
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
pub(crate) fn fire_forced_triggers(
    cx: &mut Cx,
    point: &ForcedTriggerPoint,
    bucket: EventTiming,
) -> EngineOutcome {
    // Frame-driven forced run (Slice D, #423): `resolve_one` pushes the
    // candidate's effect root frame for the global `drive` loop to own; this
    // function does not drive. Callers under the loop (effect-eval emits) get the
    // forced effect driven next; callers with post-forced work (`end_turn`'s
    // rotation, the `GameEnd` resolution finalization) arm a resumption frame
    // before emitting and let the loop drive the forced frame then re-dispatch
    // the resumption.
    //
    // At most one hit reaches here: the coordinator / emit `<2` guard routes 2+
    // simultaneous forced abilities to the ordered forced-run frame
    // (`open_forced_resolution`, #213), so there is no ordering to preserve.
    let hits = collect_forced_hits(cx.state, point, bucket);
    debug_assert!(
        hits.len() <= 1,
        "fire_forced_triggers: expected 0/1 forced hit (2+ routes through \
         open_forced_resolution); got {}",
        hits.len(),
    );
    match hits.first() {
        Some(hit) => resolve_one(cx, hit),
        None => EngineOutcome::Done,
    }
}

// dispatcher: one match arm per ForcedTriggerPoint.
#[allow(clippy::too_many_lines)]
pub(super) fn collect_forced_hits(
    state: &crate::state::GameState,
    point: &ForcedTriggerPoint,
    bucket: EventTiming,
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
            push_matching(
                reg,
                &loc.code,
                *investigator,
                None,
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::EnteredLocation),
            );
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
                    bucket,
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
                    bucket,
                    |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
                );
            }
        }
        ForcedTriggerPoint::ActAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, code, lead, None, &mut hits, bucket, |p| {
                matches!(p, EventPattern::ActAdvanced)
            });
        }
        ForcedTriggerPoint::AgendaAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(reg, code, lead, None, &mut hits, bucket, |p| {
                matches!(p, EventPattern::AgendaAdvanced)
            });
        }
        ForcedTriggerPoint::EnemyDefeated { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(reg, &act.code, lead, None, &mut hits, bucket, |p| {
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
                push_matching(reg, &act.code, lead, None, &mut hits, bucket, |p| {
                    matches!(p, EventPattern::RoundEnded)
                });
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(reg, &agenda.code, lead, None, &mut hits, bucket, |p| {
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
                        bucket,
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
                    bucket,
                    |p| matches!(p, EventPattern::EndOfTurn),
                );
            }
        }
        ForcedTriggerPoint::SkillTestResolved {
            investigator,
            kind,
            outcome,
        } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Match the card-facing narrowing: same outcome, and either an
            // unnarrowed (`None`) or kind-matching listener.
            let want = |p: &EventPattern| {
                let EventPattern::SkillTestResolved {
                    outcome: o,
                    kind: k,
                } = p
                else {
                    return false;
                };
                *o == *outcome && (k.is_none() || *k == Some(*kind))
            };
            // Scan the investigator's controlled instances (threat area + in
            // play). Bind source = the firing instance so `Effect::DiscardSelf`
            // finds itself.
            for card in inv.controlled_card_instances() {
                push_matching(
                    reg,
                    &card.code,
                    *investigator,
                    Some(card.instance_id),
                    &mut hits,
                    bucket,
                    want,
                );
            }
            // Scan the investigated location's attachment zone (Obscuring Fog
            // 01168 attaches to the location, not the threat area). Derive the
            // location from the still-live in-flight `SkillTest` frame —
            // teardown is at `PostOnResolution`, well after this fires.
            if let Some(loc_id) = state.current_skill_test().and_then(|t| t.tested_location) {
                if let Some(loc) = state.locations.get(&loc_id) {
                    for att in &loc.attachments {
                        push_matching(
                            reg,
                            &att.code,
                            *investigator,
                            Some(att.instance_id),
                            &mut hits,
                            bucket,
                            want,
                        );
                    }
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
                        bucket,
                        |p| matches!(p, EventPattern::GameEnd),
                    );
                }
            }
        }
        ForcedTriggerPoint::LeftLocation {
            investigator,
            location,
        } => {
            // Scan the left location's attachment zone (Barricade 01038);
            // bind source = the firing attachment instance for DiscardSelf.
            if let Some(loc) = state.locations.get(location) {
                for att in &loc.attachments {
                    push_matching(
                        reg,
                        &att.code,
                        *investigator,
                        Some(att.instance_id),
                        &mut hits,
                        bucket,
                        |p| matches!(p, EventPattern::LeftLocation),
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
    bucket: EventTiming,
    want: impl Fn(&EventPattern) -> bool,
) {
    let Some(abilities) = (reg.abilities_for)(code) else {
        return;
    };
    for (idx, ability) in abilities.iter().enumerate() {
        if let Trigger::OnEvent {
            pattern,
            timing,
            kind,
        } = &ability.trigger
        {
            // Forced abilities only. The coordinator scans the *same*
            // (event, bucket) for both forced and reaction (#434) — e.g. act
            // 01109 carries a `When`-`RoundEnded` *reaction* the forced scan must
            // not collect — so `kind` filtering is load-bearing, not cosmetic.
            // Scan only the bucket being resolved (the EmitEvent coordinator's
            // current cell). Today every site passes `After` except the round-end
            // `At` cell (agenda 01107 doom, Dissonant Voices 01165).
            if *kind == TriggerKind::Forced && *timing == bucket && want(pattern) {
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
    // A forced run holds only in-play / board candidates (`Hand` ⇒ `None` is
    // harmless — hand Fast events are reaction-window plays, never forced).
    let ctx =
        EvalContext::for_controller_with_optional_source(hit.controller, hit.source.instance());
    push_effect(cx, &effect, ctx);
    EngineOutcome::Done
}
