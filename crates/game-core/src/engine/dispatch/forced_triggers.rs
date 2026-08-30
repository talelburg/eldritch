//! Forced-trigger dispatch: fires `Trigger::OnEvent` abilities printed
//! on scenario-structure cards (locations, acts, agendas) at framework
//! timing points, via an immediate path separate from the player
//! reaction-window machinery. Multiple simultaneous triggers resolve in
//! a fixed deterministic order (see [`queue_forced_triggers`]), beneath the
//! universal [`queue_event`](super::emit::queue_event) chokepoint.

use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger, TriggerKind};
use crate::engine::abilities_in_effect;
use crate::state::{
    AbilitySource, CandidateSource, CardCode, EnemyId, GameState, InvestigatorId, LocationId,
    Phase, ResolutionCandidate, Status,
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
/// Wired into `move_action` (`EnteredLocation`) and all eight phase boundaries
/// (`PhaseStarted` / `PhaseEnded`, #697).
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
    /// A phase began (`Appendix_II_Timing_and_Gameplay.md` steps 1.1 / 2.1 /
    /// 3.1 / 4.1). Scans the current act and agenda for
    /// `EventPattern::PhaseStarted { phase }` forced abilities; binds
    /// controller = the lead investigator (board-wide effects ignore it).
    /// The mirror of [`PhaseEnded`](Self::PhaseEnded), sharing its scan.
    PhaseStarted { phase: Phase },
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
    /// An enemy attacked an investigator (RR p.25 step 3.3). Scans the
    /// **attacking enemy's own card** for `EventPattern::EnemyAttacks` forced
    /// abilities — Silver Twilight Acolyte 01102's *"**Forced** - After Silver
    /// Twilight Acolyte attacks: Place 1 doom on the current agenda."* Binds
    /// controller = the attacked investigator (a board-wide effect ignores it;
    /// nothing in the corpus reads it).
    ///
    /// The only forced point whose scan source is an enemy: the attack is the
    /// only triggering condition an enemy's own printed ability keys off in the
    /// Core/Dunwich corpus. Which zones a point reaches is #698's question, not
    /// this variant's.
    EnemyAttacks {
        /// The attacking enemy — the instance whose card is scanned.
        enemy: EnemyId,
        /// The investigator being attacked (controller binding).
        investigator: InvestigatorId,
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
    /// The game has ended for one **eliminated** investigator, for the purpose
    /// of resolving weakness cards — Rules Reference p.10 Elimination step 0
    /// (#638). Scans only that investigator's controlled in-play instances that
    /// are **weaknesses** (Cover Up 01007) for `EventPattern::GameEnd` forced
    /// abilities; binds controller = that investigator, source = the instance.
    ///
    /// Deliberately *not* the [`GameEnd`](Self::GameEnd) scan with a narrower
    /// input: that one skips non-`Active` investigators (#567) and fires every
    /// controlled card, both of which are wrong here.
    EliminationGameEnd {
        /// The investigator being eliminated.
        investigator: InvestigatorId,
    },
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

/// Queue the lone Forced ability matching `point`: push its effect frame (plus
/// an [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced) above
/// it in interactive mode) for the `drive` loop to resolve.
///
/// **Queues; does not resolve.** `Done` here means *the frame is on the stack*,
/// not *the effect has happened* — nothing is evaluated synchronously under the
/// frame model (#423). Callers with post-forced work resume via their own frame
/// and call this in tail position; see [`super::emit::queue_event`], the
/// chokepoint this backs, and
/// `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` (#569).
///
/// At most one hit reaches here: 2+ simultaneous forced abilities route to the
/// lead-ordered run (`open_forced_resolution`, #213, Rules Reference p.17 — the
/// player orders simultaneous triggers, even in solo), so this path has no
/// ordering to choose. Never returns `AwaitingInput`; a missing registry entry
/// at resolve time returns `Rejected`.
#[must_use = "queue_forced_triggers only pushes the forced effect's frame; the \
              effect has not run when this returns (ADR 0003)"]
pub(crate) fn queue_forced_triggers(
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
        "queue_forced_triggers: expected 0/1 forced hit (2+ routes through \
         open_forced_resolution); got {}",
        hits.len(),
    );
    match hits.first() {
        Some(hit) => {
            let out = resolve_one(cx, hit);
            // #466: in interactive play, surface the lone forced effect as a
            // one-option pick *before* it resolves. resolve_one already pushed the
            // effect root frame and returned Done; push the ack *above* it so the
            // `drive` loop hits the ack first (suspend), and on resume pops it —
            // then resolves the effect. queue_forced_triggers still returns Done
            // (push-frame contract), so emit callers stay correct. Scoped to this
            // single-hit path: the 2+ ordered run resolves via the forced-window's
            // own path (never `resolve_one`), so its ordering pick is the only
            // confirmation (no per-effect ack).
            if cx.state.interactive_acknowledge && matches!(out, EngineOutcome::Done) {
                cx.state
                    .continuations
                    .push(crate::state::Continuation::AcknowledgeForced {
                        candidate: hit.clone(),
                    });
            }
            out
        }
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
                state,
                &loc.code,
                *investigator,
                CandidateSource::Ability(AbilitySource::Location(*location)),
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::EnteredLocation),
            );
        }
        ForcedTriggerPoint::PhaseStarted { phase } => {
            let want_phase = dsl_phase(*phase);
            push_scenario_structure_matching(
                state,
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::PhaseStarted { phase } if *phase == want_phase),
            );
        }
        ForcedTriggerPoint::PhaseEnded { phase } => {
            let want_phase = dsl_phase(*phase);
            push_scenario_structure_matching(
                state,
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::PhaseEnded { phase } if *phase == want_phase),
            );
        }
        ForcedTriggerPoint::ActAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(
                state,
                code,
                lead,
                CandidateSource::Ability(AbilitySource::Act),
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::ActAdvanced),
            );
        }
        ForcedTriggerPoint::AgendaAdvanced { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            push_matching(
                state,
                code,
                lead,
                CandidateSource::Ability(AbilitySource::Agenda),
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::AgendaAdvanced),
            );
        }
        ForcedTriggerPoint::EnemyDefeated { code } => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(
                    state,
                    &act.code,
                    lead,
                    CandidateSource::Ability(AbilitySource::Act),
                    &mut hits,
                    bucket,
                    |p| {
                        matches!(
                            p,
                            EventPattern::EnemyDefeated { code: narrow, .. }
                                if narrow.as_deref().is_none_or(|c| c == code.as_str())
                        )
                    },
                );
            }
        }
        ForcedTriggerPoint::EnemyAttacks {
            enemy,
            investigator,
        } => {
            let Some(attacker) = state.enemies.get(enemy) else {
                // The attacker was removed mid-sequence (a `when` reaction
                // defeated it). Nothing to scan; the cell is simply empty.
                return hits;
            };
            push_matching(
                state,
                &attacker.code,
                *investigator,
                CandidateSource::Ability(AbilitySource::Enemy(*enemy)),
                &mut hits,
                bucket,
                |p| matches!(p, EventPattern::EnemyAttacks),
            );
        }
        ForcedTriggerPoint::RoundEnded => {
            let Some(lead) = state.turn_order.first().copied() else {
                return hits;
            };
            if let Some(act) = state.act_deck.get(state.act_index) {
                push_matching(
                    state,
                    &act.code,
                    lead,
                    CandidateSource::Ability(AbilitySource::Act),
                    &mut hits,
                    bucket,
                    |p| matches!(p, EventPattern::RoundEnded),
                );
            }
            if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
                push_matching(
                    state,
                    &agenda.code,
                    lead,
                    CandidateSource::Ability(AbilitySource::Agenda),
                    &mut hits,
                    bucket,
                    |p| matches!(p, EventPattern::RoundEnded),
                );
            }
            // Persistent threat-area treacheries discard on RoundEnded
            // (Dissonant Voices 01165). Scan every investigator's
            // controlled instances; bind source = the instance so
            // `Effect::DiscardSelf` finds itself.
            //
            // Skip eliminated investigators: Rules Reference p.10 Elimination
            // removes their cards from the game (step 1) / to the encounter
            // discard (step 4), so their in-play instances are already gone —
            // except `investigator_card`, which is a non-Option identity/harm
            // field (#448) that cannot be drained. This filter is that card's
            // only guard (#567). `investigators` is a BTreeMap, so filtering
            // preserves the frozen enumeration order (#570).
            for (inv_id, inv) in state
                .investigators
                .iter()
                .filter(|(_, inv)| inv.status == Status::Active)
            {
                for card in inv.controlled_card_instances() {
                    push_matching(
                        state,
                        &card.code,
                        *inv_id,
                        CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
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
                    state,
                    &card.code,
                    *investigator,
                    CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
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
                    state,
                    &card.code,
                    *investigator,
                    CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
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
                            state,
                            &att.code,
                            *investigator,
                            CandidateSource::Ability(AbilitySource::InPlay(att.instance_id)),
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
            //
            // Skip eliminated investigators: Rules Reference p.10 Elimination
            // removes their cards from the game (step 1) / to the encounter
            // discard (step 4), so their in-play instances are already gone —
            // except `investigator_card`, which is a non-Option identity/harm
            // field (#448) that cannot be drained. This filter is that card's
            // only guard (#567). Filtering the BTreeMap preserves the frozen
            // enumeration order (#570).
            for (inv_id, inv) in state
                .investigators
                .iter()
                .filter(|(_, inv)| inv.status == Status::Active)
            {
                for card in inv.controlled_card_instances() {
                    push_matching(
                        state,
                        &card.code,
                        *inv_id,
                        CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
                        &mut hits,
                        bucket,
                        |p| matches!(p, EventPattern::GameEnd),
                    );
                }
            }
        }
        ForcedTriggerPoint::EliminationGameEnd { investigator } => {
            let Some(inv) = state.investigators.get(investigator) else {
                return hits;
            };
            // Rules Reference p.10 Elimination step 0: *"Trigger any 'when the
            // game ends' abilities on each weakness the eliminated investigator
            // owns that is in play."* Two narrowings the ordinary `GameEnd` scan
            // does not make:
            //
            // - **weaknesses only.** A non-weakness card this investigator
            //   controls has no game-end trigger point here — the game has not
            //   ended for anyone else. `metadata_for` answers for the corpus, so
            //   a card with no metadata is not a weakness and is skipped.
            //
            //   The rule says *owns*; this iterates what the investigator
            //   **controls**. The two coincide for every weakness the engine can
            //   represent today: a weakness enters its own owner's threat area,
            //   and nothing models one player controlling another's card. RR p.10
            //   step 1 already carries the sub-clause that would separate them
            //   ("Any card that player owns but does not control…"), so if
            //   cross-player control ever lands, this scan needs an ownership
            //   field to filter on rather than a re-reading.
            // - **no `Status` filter.** `apply_investigator_elimination` flips status
            //   before running the steps, so the investigator this point names is
            //   never `Active` — filtering on it (as `GameEnd`/`RoundEnded` do,
            //   #567) would drop every hit.
            for card in inv
                .controlled_card_instances()
                .filter(|c| (reg.metadata_for)(&c.code).is_some_and(|m| m.weakness))
            {
                push_matching(
                    state,
                    &card.code,
                    *investigator,
                    CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
                    &mut hits,
                    bucket,
                    |p| matches!(p, EventPattern::GameEnd),
                );
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
                        state,
                        &att.code,
                        *investigator,
                        CandidateSource::Ability(AbilitySource::InPlay(att.instance_id)),
                        &mut hits,
                        bucket,
                        |p| matches!(p, EventPattern::LeftLocation),
                    );
                }
            }
        }
    }
    // RR p.2: a forced ability that lacks the potential to change the game state
    // does not initiate. Drop such hits here — the single chokepoint feeding both
    // the lone-hit path (`queue_forced_triggers`) and the 2+ ordered run
    // (`open_forced_resolution`) — so a no-op forced neither resolves nor (post-
    // #466) prompts. Conservative: only provable no-ops are dropped (#495).
    //
    // The gate is the same predicate the reaction side offers on, so a forced
    // ability's `eligibility` tag counts here too (#786): Cover Up 01007's
    // "if there are any clues on Cover Up" lives in an opaque native effect the
    // generic check can't introspect, and without the tag layer a clueless Cover
    // Up initiated — and prompted — at game end.
    hits.retain(|hit| {
        let Some(abilities) =
            abilities_in_effect::for_candidate_source(state, hit.source, &hit.code)
        else {
            return false;
        };
        let Some((_, ability)) = abilities.iter().find(|(addr, _)| *addr == hit.address) else {
            return false;
        };
        crate::engine::evaluator::ability_can_initiate(state, ability, hit.source, hit.controller)
    });
    hits
}

/// Scan the current act and the current agenda for forced abilities matching
/// `want`, binding controller = the lead investigator.
///
/// The scan both phase-boundary points share: the milestone is board-wide, so
/// the controller it binds is the lead (first of `turn_order`) and the
/// board-wide effects that key off a phase boundary ignore it. An empty
/// `turn_order` finds nothing rather than panicking — a scenario with no seated
/// investigator has no lead to bind.
///
/// **Act and agenda only.** An enemy or asset in play printing a phase-boundary
/// Forced — Wizard of the Order 01170, Hunting Horror 02141, Peter Clover
/// 02079 — is not reached, because there is no doom-on-a-card model for the
/// first of them to place onto and no Dunwich corpus for the other two. #792
/// carries the doom model, this scan's in-play arm, and 01170 together, since
/// none of the three is assertable without the other two.
fn push_scenario_structure_matching(
    state: &crate::state::GameState,
    hits: &mut Vec<ResolutionCandidate>,
    bucket: EventTiming,
    want: impl Fn(&EventPattern) -> bool + Copy,
) {
    let Some(lead) = state.turn_order.first().copied() else {
        return;
    };
    if let Some(act) = state.act_deck.get(state.act_index) {
        push_matching(
            state,
            &act.code,
            lead,
            CandidateSource::Ability(AbilitySource::Act),
            hits,
            bucket,
            want,
        );
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        push_matching(
            state,
            &agenda.code,
            lead,
            CandidateSource::Ability(AbilitySource::Agenda),
            hits,
            bucket,
            want,
        );
    }
}

/// Map the engine's `state::Phase` to the `card-dsl` mirror so a
/// `PhaseStarted` / `PhaseEnded` pattern can be compared.
fn dsl_phase(phase: Phase) -> crate::dsl::Phase {
    match phase {
        Phase::Mythos => crate::dsl::Phase::Mythos,
        Phase::Investigation => crate::dsl::Phase::Investigation,
        Phase::Enemy => crate::dsl::Phase::Enemy,
        Phase::Upkeep => crate::dsl::Phase::Upkeep,
    }
}

fn push_matching(
    state: &GameState,
    code: &CardCode,
    controller: InvestigatorId,
    source: CandidateSource,
    out: &mut Vec<ResolutionCandidate>,
    bucket: EventTiming,
    want: impl Fn(&EventPattern) -> bool,
) {
    let Some(abilities) = abilities_in_effect::for_candidate_source(state, source, code) else {
        return;
    };
    for (address, ability) in &abilities {
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
            // current cell). Since #702 every condition walks all three, so a
            // card is collected in the cell its printed trigger word names.
            if *kind == TriggerKind::Forced && *timing == bucket && want(pattern) {
                out.push(ResolutionCandidate {
                    code: code.clone(),
                    controller,
                    address: address.clone(),
                    // Origin set by the caller: an in-play / threat-area instance,
                    // a scenario board card, or a location's own forced ability.
                    source,
                });
            }
        }
    }
}

fn resolve_one(cx: &mut Cx, hit: &ResolutionCandidate) -> EngineOutcome {
    if card_registry::current().is_none() {
        return EngineOutcome::Rejected {
            reason: "queue_forced_triggers: registry vanished between collect and resolve".into(),
        };
    }
    let Some(ability) = abilities_in_effect::resolve(cx.state, hit.source, &hit.code, &hit.address)
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "queue_forced_triggers: {} no longer has the ability at {:?} at resolve time",
                hit.code, hit.address,
            )
            .into(),
        };
    };
    let effect = ability.effect;
    // A forced run holds only in-play / board candidates (`Hand` ⇒ `None` is
    // harmless — hand Fast events are reaction-window plays, never forced).
    let ctx =
        EvalContext::for_controller_with_optional_source(hit.controller, hit.source.instance());
    push_effect(cx, &effect, ctx);
    EngineOutcome::Done
}

/// Display name for the card a forced ability is printed on, for the
/// [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced) prompt.
/// Resolved via the registry; falls back to the raw code when no
/// registry/metadata is available (tests).
fn forced_source_name(code: &CardCode) -> String {
    crate::card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map_or_else(|| code.0.clone(), |m| m.name.clone())
}

/// Drive a [`Continuation::AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced)
/// frame (#466): suspend with a one-option `PickSingle` naming the source. The
/// pick precedes the forced effect's resolution ("confirm before the effect").
/// Mirrors `advance_reverse::drive`'s `AwaitAck` suspend.
pub(crate) fn drive_acknowledge_forced(cx: &mut Cx) -> EngineOutcome {
    use crate::engine::{ChoiceOption, InputRequest, OptionId, ResumeToken};
    let Some(crate::state::Continuation::AcknowledgeForced { candidate }) =
        cx.state.continuations.last()
    else {
        return EngineOutcome::Rejected {
            reason: "drive_acknowledge_forced: top frame is not AcknowledgeForced".into(),
        };
    };
    let name = forced_source_name(&candidate.code);
    let anchor = super::reaction_windows::candidate_anchor(candidate);
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            format!("Forced — {name}"),
            vec![ChoiceOption::new(OptionId(0), "Resolve").at(anchor)],
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume an [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced)
/// frame: validate the single option, pop the frame, and return `Done` so the
/// `drive` loop resolves the forced effect beneath.
pub(crate) fn resume_acknowledge_forced(
    cx: &mut Cx,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    use crate::engine::OptionId;
    if !matches!(
        response,
        crate::action::InputResponse::PickSingle(OptionId(0))
    ) {
        return EngineOutcome::Rejected {
            reason: "resume_acknowledge_forced: expected the single forced-resolution option"
                .into(),
        };
    }
    debug_assert!(matches!(
        cx.state.continuations.last(),
        Some(crate::state::Continuation::AcknowledgeForced { .. })
    ));
    cx.state.continuations.pop();
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AbilityAddress, CardInstanceId};

    #[test]
    fn acknowledge_forced_suspends_then_pops_on_pick() {
        use crate::action::InputResponse;
        use crate::engine::OptionId;
        use crate::state::Continuation;
        use crate::test_support::GameStateBuilder;

        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(1))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // Drive: one-option suspend.
        let out = super::drive_acknowledge_forced(&mut cx);
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }

        // Resume with the single option: frame pops, returns Done.
        let out =
            super::resume_acknowledge_forced(&mut cx, &InputResponse::PickSingle(OptionId(0)));
        assert!(matches!(out, EngineOutcome::Done));
        assert!(
            cx.state.continuations.is_empty(),
            "the AcknowledgeForced frame must be popped on resume"
        );
    }

    #[test]
    fn acknowledge_forced_rejects_non_pick_response() {
        use crate::action::InputResponse;
        use crate::state::Continuation;
        use crate::test_support::GameStateBuilder;

        // Validate-first: a Confirm/Skip (not the single PickSingle) is rejected
        // and leaves the frame in place.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(1))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = super::resume_acknowledge_forced(&mut cx, &InputResponse::Confirm);
        assert!(matches!(out, EngineOutcome::Rejected { .. }));
        assert!(
            matches!(
                cx.state.continuations.last(),
                Some(Continuation::AcknowledgeForced { .. })
            ),
            "a rejected resume must leave the frame in place"
        );
    }

    #[test]
    fn acknowledge_forced_anchors_the_option_to_its_source_card() {
        use crate::engine::OptionTarget;
        use crate::state::Continuation;
        use crate::test_support::GameStateBuilder;

        // A forced ability on an in-play instance surfaces a one-option pick
        // anchored to that card (#553), not Global.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01020"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(5))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(
                    request.options[0].target,
                    Some(OptionTarget::CardInstance(CardInstanceId(5))),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn acknowledge_forced_anchors_a_location_source_to_its_map_node() {
        use crate::engine::OptionTarget;
        use crate::state::{Continuation, LocationId};
        use crate::test_support::GameStateBuilder;

        // A location's own forced ability (the Attic's on-enter horror) surfaces a
        // one-option pick anchored to the location on the map (#553), not Global.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(3))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(
                    request.options[0].target,
                    Some(OptionTarget::Location(LocationId(3))),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn acknowledge_forced_anchors_an_agenda_source_to_the_agenda_card() {
        use crate::engine::OptionTarget;
        use crate::state::{Agenda, Continuation};
        use crate::test_support::GameStateBuilder;

        // A forced ability on the current agenda (What's Going On?! 01105's
        // on-advance reverse) anchors its "Resolve" to the agenda card (#556).
        let mut state = GameStateBuilder::default().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01105"),
            doom_threshold: 3,
        }];
        state.agenda_index = 0;
        state.continuations.push(Continuation::AcknowledgeForced {
            candidate: ResolutionCandidate::new(
                CardCode::new("01105"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Agenda),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match super::drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(request.options[0].target, Some(OptionTarget::Agenda));
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }
}
