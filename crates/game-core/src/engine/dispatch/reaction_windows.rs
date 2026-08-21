//! Reaction-window and fast-window helpers.
//!
//! Contains the open/scan/fire/close pipeline for after-event reaction
//! windows ([`scan_pending_triggers`],
//! [`trigger_matches`], [`open_queued_reaction_window`],
//! [`resume_reaction_window`], [`fire_pending_trigger`],
//! [`bump_usage_counter`], [`close_reaction_window`]) and the fast-window
//! eligibility checks
//! ([`check_play_card`], [`check_activate_ability`],
//! [`any_fast_play_eligible`], [`open_fast_window`]).

use std::borrow::Cow;

use crate::action::InputResponse;
use crate::card_data::{CardMetadata, CardType};
use crate::card_registry;
use crate::dsl::{Ability, ActionDesignator, EventPattern, EventTiming, Trigger, TriggerKind};
use crate::engine::enumerate::TurnAction;
use crate::engine::TimingEvent;
use crate::event::{Event, LapseReason};
use crate::state::TimingMode;
use crate::state::{
    AbilitySource, CandidateSource, CardCode, Continuation, FastActorScope, FastWindowKind,
    GameState, InvestigatorId, Phase, ResolutionCandidate, Status,
};

use super::super::evaluator::{push_effect, EvalContext};
use super::super::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use super::Cx;

/// Push a reaction window frame for `candidates` at `bucket`. The shared push
/// behind [`open_reaction_run`] (which queues and then opens) and the coordinator's
/// per-cell scan.
///
/// Reaction windows admit any investigator's Fast actions (RR: Fast may be
/// played at any player window) — encoded by `mode: Reaction` (the former
/// `FastActorScope::Any` binding). Multi-window nesting is structural.
fn push_reaction_window(
    cx: &mut Cx,
    event: &crate::engine::TimingEvent,
    bucket: EventTiming,
    candidates: Vec<ResolutionCandidate>,
) {
    cx.state
        .continuations
        .push(Continuation::TimingPointWindow {
            event: event.clone(),
            bucket,
            mode: crate::state::TimingMode::Reaction,
            candidates,
        });
}

/// All reaction candidates (in-play + hand Fast + current act/agenda) for
/// `event` at `bucket` — the `EmitEvent`/`TimingPoint` coordinator's per-cell
/// reaction scan (#434). The caller names the cell it is resolving; a cell is
/// populated iff this finds something in it (#702 deleted the per-event table of
/// whether a condition opens a reaction window at all).
pub(super) fn scan_reactions_at(
    state: &GameState,
    event: &crate::engine::TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let mut candidates = scan_pending_triggers(state, event, bucket);
    candidates.extend(scan_hand_fast_events(state, event, bucket));
    candidates
}

/// Push a reaction window for the coordinator's pre-scanned `candidates` and
/// open it (the round-end `when` act-advance window, #434). `bucket` is the cell
/// the caller scanned, recorded on the frame so the fire-time re-validation can
/// re-scan the same cell (#568). Returns the `AwaitingInput` from
/// [`open_queued_reaction_window`]. Caller guarantees `candidates` is non-empty
/// (it checked, to decide open-vs-finish).
///
/// This is also the one path where re-validation is provably a no-op — the caller
/// scanned this cell moments ago and nothing between then and here mutates what
/// the scan reads — so it carries the debug-only tripwire for #568's re-scan. A
/// withdrawal *here* means [`scan_reactions_at`] disagrees with itself over
/// unchanged state, which is a bug in the scan, not in the re-check. The other
/// prompt site can't assert this: its window was queued an emit ago, and the
/// forced abilities that resolved in between are entitled to have withdrawn
/// something.
pub(super) fn open_reaction_run(
    cx: &mut Cx,
    event: &crate::engine::TimingEvent,
    bucket: EventTiming,
    candidates: Vec<ResolutionCandidate>,
) -> EngineOutcome {
    debug_assert!(
        !candidates.is_empty(),
        "open_reaction_run: caller must pass a non-empty candidate list"
    );
    push_reaction_window(cx, event, bucket, candidates);
    #[cfg(debug_assertions)]
    {
        let withdrawn = withdraw_lapsed_candidates(cx);
        assert_eq!(
            withdrawn, 0,
            "open_reaction_run: re-validating the cell the caller just scanned withdrew \
             {withdrawn} candidate(s) — the reaction scan is not idempotent over unchanged \
             state (#568)",
        );
    }
    open_queued_reaction_window(cx)
}

/// Open the forced-resolution run (Axis-B T5b / #213): push a
/// `TimingPointWindow { mode: Forced }` holding the 2+ simultaneous forced
/// `candidates`, and present the lead investigator's order choice. The forced
/// run is mandatory (cannot be skipped) and admits no Fast plays. It carries no
/// resume continuation (#434): on close it returns `Done` and the `drive` loop
/// re-dispatches the exposed parent frame. The caller returns the `AwaitingInput`.
///
/// `bucket` is the cell the caller collected at. The frame is the same variant a
/// reaction window uses, so every one records its cell; only the reaction path
/// reads it back, to re-validate (#568 — and TODO(#607) for this path).
pub(super) fn open_forced_resolution(
    cx: &mut Cx,
    event: &crate::engine::TimingEvent,
    bucket: EventTiming,
    candidates: Vec<ResolutionCandidate>,
) -> EngineOutcome {
    cx.state
        .continuations
        .push(Continuation::TimingPointWindow {
            event: event.clone(),
            bucket,
            mode: crate::state::TimingMode::Forced,
            candidates,
        });
    open_queued_reaction_window(cx)
}

/// Whether investigators `a` and `b` share a (revealed) current location.
/// Used by the before-attack cancel window's "at your location" scoping
/// (Axis D #336); two investigators between locations (`None`) never match.
fn same_location(state: &GameState, a: InvestigatorId, b: InvestigatorId) -> bool {
    let loc = |id| {
        state
            .investigators
            .get(&id)
            .and_then(|i| i.current_location)
    };
    loc(a).is_some_and(|la| loc(b) == Some(la))
}

/// Whether a reaction `ability` may be offered, per its
/// [`Ability::eligibility`] tag (RR p.2: an ability can't initiate if its
/// effect won't change game state). Pure over `&GameState`, which is what lets
/// [`withdraw_lapsed_candidates`] re-ask it once a sibling option has resolved
/// (#568). No tag → eligible. A tag with no
/// resolvable predicate (registry absent / unknown tag) → suppressed, so a
/// half-installed host never offers a gated reaction it can't evaluate.
fn ability_eligible(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    let ctx = EvalContext::for_controller_with_optional_source(controller, source.instance());
    // Generic RR p.2/p.3 initiation gate: the effect must have the potential to
    // change the game state (Roland 01001's clue discovery at a 0-clue location,
    // #495). Conservative — only provable no-ops are suppressed.
    if !crate::engine::evaluator::effect_can_change_state(state, ctx, &ability.effect) {
        return false;
    }
    // The native eligibility tag refines opaque `Native` effects the generic gate
    // can't introspect (#368: Cover Up 01007, act 01109). No tag → eligible.
    let Some(tag) = ability.eligibility.as_deref() else {
        return true;
    };
    let Some(reg) = card_registry::current() else {
        return false;
    };
    let Some(pred) = (reg.native_eligibility_for)(tag) else {
        return false;
    };
    pred(state, &ctx)
}

/// Scan every investigator's `cards_in_play` **and the current act/agenda** for
/// `Trigger::OnEvent` reaction abilities matching `event` whose `EventTiming`
/// equals `bucket`, building a pending-trigger list in active-investigator-first
/// / turn-order resolution order (act/agenda board candidates, controlled by the
/// lead, appended last).
///
/// The `bucket` filter is what lets the coordinator scan one timing cell at a
/// time (#434): on `RoundEnded`, `When` surfaces act 01109's group advance while
/// `At`/`After` surface nothing (its doom is *forced*, not a reaction). Since
/// #702 every condition is scanned this way; the three that still bypass the
/// coordinator pass the one cell they open at.
///
/// Returns an empty vec when the registry isn't installed (tests that
/// don't touch card data) or no cards match.
fn scan_pending_triggers(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    // Active investigator first, then the rest of turn_order in their
    // listed order. Investigators not in turn_order are skipped
    // entirely — a bare plain skill-test path can run without a
    // turn order populated, but no scenario opens a reaction window
    // outside an action initiated by a turn-order investigator.
    let mut order: Vec<InvestigatorId> = Vec::with_capacity(state.turn_order.len());
    if let Some(active) = state.active_investigator {
        order.push(active);
    }
    for id in &state.turn_order {
        if Some(*id) != state.active_investigator {
            order.push(*id);
        }
    }

    let mut pending: Vec<ResolutionCandidate> = Vec::new();
    for id in order {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        // "at your location" scoping for the before-attack cancel window
        // (Dodge 01023, Axis D #336): a candidate's controller must be
        // co-located with the attacked investigator. Other events pass all
        // controllers through.
        if let TimingEvent::EnemyAttacks { investigator, .. } = event {
            if !same_location(state, id, *investigator) {
                continue;
            }
        }
        // "…YOU … at YOUR location" (Cover Up 01007, Axis D #336): a candidate's
        // controller is the discoverer and must be at the discovery location.
        // Applies in every cell — the scoping is the condition's, not the
        // interrupt's. (The per-card `clues > 0` potential gate is in the card
        // loop below.)
        if let TimingEvent::DiscoverClues {
            investigator,
            location,
            ..
        } = event
        {
            if id != *investigator
                || state
                    .investigators
                    .get(&id)
                    .and_then(|i| i.current_location)
                    != Some(*location)
            {
                continue;
            }
        }
        for card in inv.controlled_card_instances() {
            // Self-binding: for `DamageAssigned` only an asset the assignment
            // gives damage to may trigger. All other instances are skipped here
            // — the pattern match in `trigger_matches` handles the pattern
            // pairing and the "an enemy attack" narrowing; this filter enforces
            // the "self = a card being dealt damage" scoping (Guard Dog 01021).
            // It reads the *event's* assignment, not the frame's, which is what
            // makes an edit in a `when` cell visible to the cells after it
            // without a write-back protocol (ADR 0009). Other events pass all
            // instances through.
            if let TimingEvent::DamageAssigned { assignment, .. } = event {
                if !assignment.asset_damage.contains_key(&card.instance_id) {
                    continue;
                }
            }
            // Self-binding: `EnteredPlay` fires only for the instance that
            // entered play (Research Librarian 01032). Mirrors the soaked-asset
            // filter above.
            if let TimingEvent::EnteredPlay { instance, .. } = event {
                if card.instance_id != *instance {
                    continue;
                }
            }
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (idx, ability) in abilities.iter().enumerate() {
                let Trigger::OnEvent {
                    pattern,
                    timing,
                    kind,
                } = &ability.trigger
                else {
                    continue;
                };
                // Reaction abilities only, at the cell being scanned (#434): the
                // coordinator scans the same (event, bucket) for both forced and
                // reaction, so kind filtering keeps a Forced ability out of the
                // reaction window (symmetric to push_matching). For single-bucket
                // events `bucket` is the event's natural timing — behaviour-preserving.
                if *kind != TriggerKind::Reaction || *timing != bucket {
                    continue;
                }
                if !trigger_matches(event, pattern, id) {
                    continue;
                }
                let ability_index = u8::try_from(idx)
                    .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
                // "Limit X per [period]" — skip triggers whose per-
                // instance counter has already reached the cap this
                // round. Rules Reference page 14.
                if card.is_usage_exhausted(ability_index, ability.usage_limit, state.round) {
                    continue;
                }
                // Eligibility gate (RR p.2): suppress a reaction whose effect
                // can't change state (e.g. an emptied Cover Up 01007).
                if !ability_eligible(
                    state,
                    ability,
                    CandidateSource::InPlay(card.instance_id),
                    id,
                ) {
                    continue;
                }
                // Reaction candidates always have a source instance — an
                // in-play / threat-area card, or the investigator card itself
                // (#448 cp3a, now folded into `controlled_card_instances()`);
                // abilities resolve by `code`. `bump_usage_counter` resolves
                // the instance against all three zones.
                pending.push(ResolutionCandidate {
                    code: card.code.clone(),
                    controller: id,
                    ability_index,
                    source: CandidateSource::InPlay(card.instance_id),
                });
            }
        }
    }
    pending.extend(scan_act_agenda_reactions(state, event, bucket));
    pending
}

/// Scan the current act + agenda for `Trigger::OnEvent` reaction abilities
/// matching `event` at `bucket` — act 01109's "When the round ends,
/// investigators … may … advance" group window (#434). The act/agenda are not
/// in any `cards_in_play` zone, so [`scan_pending_triggers`] can't reach them in
/// its per-investigator loop. Mirrors `collect_forced_hits`'s act/agenda scan:
/// controller = the lead (board-wide effects ignore it), `CandidateSource::Board`,
/// no per-instance usage cap (acts have none). Empty when the registry isn't
/// installed or nothing matches.
fn scan_act_agenda_reactions(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let Some(lead) = state.turn_order.first().copied() else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for code in [
        state.act_deck.get(state.act_index).map(|a| &a.code),
        state.agenda_deck.get(state.agenda_index).map(|a| &a.code),
    ]
    .into_iter()
    .flatten()
    {
        let Some(abilities) = (reg.abilities_for)(code) else {
            continue;
        };
        for (idx, ability) in abilities.iter().enumerate() {
            let Trigger::OnEvent {
                pattern,
                timing,
                kind,
            } = &ability.trigger
            else {
                continue;
            };
            if *kind != TriggerKind::Reaction
                || *timing != bucket
                || !trigger_matches(event, pattern, lead)
            {
                continue;
            }
            // Eligibility gate (RR p.2): suppress an act/agenda reaction whose
            // effect can't change state (e.g. The Barrier 01109's round-end
            // advance when the Hallway group can't afford the clue threshold).
            if !ability_eligible(state, ability, CandidateSource::Board, lead) {
                continue;
            }
            let ability_index = u8::try_from(idx)
                .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
            hits.push(ResolutionCandidate {
                code: code.clone(),
                controller: lead,
                ability_index,
                source: CandidateSource::Board,
            });
        }
    }
    hits
}

/// Scan every window-eligible investigator's hand for Fast **events** whose
/// `Trigger::OnEvent` ability matches `kind` (Axis C, #335). The play-timing
/// predicate is the same [`trigger_matches`] used for in-play reactions — per
/// Rules Reference p.11 a Fast reaction event plays "as if the described
/// timing point were a triggering condition", so a hand Fast event is its
/// in-play twin sourced from hand.
///
/// Returns [`CandidateSource::Hand`] candidates in active-investigator-first
/// / turn-order order, like [`scan_pending_triggers`]. Empty when the registry
/// isn't installed (tests that don't touch card data) or nothing matches.
fn scan_hand_fast_events(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    let mut order: Vec<InvestigatorId> = Vec::with_capacity(state.turn_order.len());
    if let Some(active) = state.active_investigator {
        order.push(active);
    }
    for id in &state.turn_order {
        if Some(*id) != state.active_investigator {
            order.push(*id);
        }
    }

    let mut plays = Vec::new();
    for id in order {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        // "at your location" scoping for the before-attack cancel window —
        // mirrors `scan_pending_triggers` (Dodge 01023, Axis D #336).
        if let TimingEvent::EnemyAttacks { investigator, .. } = event {
            if !same_location(state, id, *investigator) {
                continue;
            }
        }
        for code in &inv.hand {
            let Some(meta) = (reg.metadata_for)(code) else {
                continue;
            };
            if !meta.is_fast() || meta.card_type() != CardType::Event {
                continue;
            }
            let Some(abilities) = (reg.abilities_for)(code) else {
                continue;
            };
            for (idx, ability) in abilities.iter().enumerate() {
                let Trigger::OnEvent {
                    pattern,
                    timing,
                    kind,
                } = &ability.trigger
                else {
                    continue;
                };
                // Reaction abilities only, at the cell being scanned (#434): the
                // coordinator scans the same (event, bucket) for both forced and
                // reaction, so kind filtering keeps a Forced ability out of the
                // reaction window (symmetric to push_matching). For single-bucket
                // events `bucket` is the event's natural timing — behaviour-preserving.
                if *kind != TriggerKind::Reaction || *timing != bucket {
                    continue;
                }
                if !trigger_matches(event, pattern, id) {
                    continue;
                }
                // RR initiation gate: a Fast event can't be played if its effect
                // can't change game state — same rule as the in-play reaction scan
                // (#495). Covers Evidence! 01022 (Roland's reaction sourced from
                // hand: discover 1 clue at your location) at a 0-clue location.
                if !ability_eligible(state, ability, CandidateSource::Hand, id) {
                    continue;
                }
                // RR p.22 affordability: don't offer a Fast event whose resource
                // cost can't be paid (Evidence! 01022 costs 1; not offered at 0
                // resources). The play path (play_fast_event) pays it (#501).
                // Filtering here keeps the offer honest; it is not the binding
                // check — the wallet is shared, so a sibling option can empty it
                // after this ran, and initiation re-asks (#568).
                if check_play_resource_cost_payable(state, id, code).is_err() {
                    continue;
                }
                let ability_index = u8::try_from(idx)
                    .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
                plays.push(ResolutionCandidate {
                    code: code.clone(),
                    controller: id,
                    ability_index,
                    source: CandidateSource::Hand,
                });
                // One option per card: a card with two matching abilities is
                // still offered once. No in-scope card has two.
                break;
            }
        }
    }
    plays
}

/// Returns whether an [`Trigger::OnEvent`] ability with the given
/// `pattern` and `timing`, owned by `controller`, matches a window of
/// the given `kind`.
///
/// Phase-3 mapping:
/// - the after-enemy-defeated reaction window
///   ([`TimingEvent::EnemyDefeated`]) matches
///   [`EventPattern::EnemyDefeated`] with
///   [`EventTiming::After`]. The `by_controller` qualifier narrows to
///   defeats credited to this ability's controller.
///
/// **Timing is not consulted here** (#704). Which cell an ability resolves in is
/// the coordinator's business — it scans one cell at a time and `push_matching` /
/// the scans filter on `timing == bucket` — so a pattern pairs with its condition
/// identically in all three cells. The former `When` whitelist of
/// condition/pattern pairs permitted to carry interrupt timing is gone with the
/// last single-cell condition; an interrupt declared on a condition that cannot
/// honour it is rejected loudly by the coordinator's caller-owned `when` arm
/// rather than silently failing to match here.
fn trigger_matches(
    event: &TimingEvent,
    pattern: &EventPattern,
    controller: InvestigatorId,
) -> bool {
    match (event, pattern) {
        (
            TimingEvent::EnemyDefeated { by, .. },
            EventPattern::EnemyDefeated {
                by_controller,
                code: _,
            },
        ) => {
            if *by_controller {
                *by == Some(controller)
            } else {
                true
            }
        }
        // Three pairings whose narrowing is entirely someone else's: the pattern
        // matching its condition is the whole answer here.
        //
        // - "an enemy attacks an investigator at your location" — Dodge 01023 in
        //   the `when` cell, Silver Twilight Acolyte 01102 in the `after` one.
        //   The co-location narrowing lives in the scans, which have the board.
        // - "When the round ends, investigators … may … advance" — act 01109's
        //   group advance (#434). Board-scoped; the contributor scoping lives in
        //   the native and in the round-end coordinator's `when` cell.
        (TimingEvent::EnemyAttacks { .. }, EventPattern::EnemyAttacks)
        | (TimingEvent::RoundEnded, EventPattern::RoundEnded) => true,
        // "**When an enemy attack** deals damage to Guard Dog" (01021). The
        // self-binding half — that this card is one the assignment gives damage
        // to — is the instance filter in `scan_pending_triggers`, so only such
        // an instance reaches here; what is left is the card's own narrowing of
        // the condition to an enemy attack, which `Effect::Deal` harm (Dynamite
        // Blast, a treachery) does not satisfy.
        (TimingEvent::DamageAssigned { source, .. }, EventPattern::EnemyAttackDamagedSelf) => {
            matches!(source, crate::state::DamageSource::EnemyAttack { .. })
        }
        // "after you succeed/fail a skill test" — scoped to the controller's
        // own test ("after **you** …"), narrowed by outcome and (optionally)
        // test kind. Dr. Milan 01033 is `{ Success, Some(Investigate) }`.
        (
            TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome,
            },
            EventPattern::SkillTestResolved {
                outcome: p_out,
                kind: p_kind,
            },
        ) => {
            *investigator == controller
                && outcome == p_out
                && (p_kind.is_none() || *p_kind == Some(*kind))
        }
        // "…you discover clues": scoped to the discovering investigator, the way
        // the `when` pairing above is. The "at your location" narrowing lives in
        // the scan, which has the board (#703).
        (TimingEvent::DiscoverClues { investigator, .. }, EventPattern::DiscoverClues) => {
            *investigator == controller
        }
        // Scoped to the entered card's owner; the self-instance scoping is in
        // the scan (Research Librarian 01032).
        (
            TimingEvent::EnteredPlay {
                controller: window_controller,
                ..
            },
            EventPattern::EnteredPlay,
        ) => *window_controller == controller,
        // Every other (event, pattern) pairing opens no reaction: the
        // forced-only conditions (PhaseEnded / ActAdvanced / AgendaAdvanced /
        // EndOfTurn / GameEnd / EliminationGameEnd / EnteredLocation /
        // LeftLocation) never open a reaction window.
        _ => false,
    }
}

/// The current act's card code, if an act deck is loaded — used to anchor the
/// round-end act-advance reaction (a [`CandidateSource::Board`] candidate whose
/// code is the act) to the act card (S5, #540). `None` for fixtures with no act.
pub(super) fn current_act_code(state: &GameState) -> Option<CardCode> {
    state
        .act_deck
        .get(state.act_index)
        .map(|act| act.code.clone())
}

/// The current agenda's printed code, if the agenda deck is non-empty. The mirror
/// of [`current_act_code`]; anchors an agenda-sourced forced effect to the agenda
/// card (#556). Correct at forced-ack time even for an `AgendaAdvanced` reverse:
/// the forced fires at `FireReverse`, before `Finalize` bumps `agenda_index`.
pub(super) fn current_agenda_code(state: &GameState) -> Option<CardCode> {
    state
        .agenda_deck
        .get(state.agenda_index)
        .map(|agenda| agenda.code.clone())
}

/// The board anchor for a resolution candidate's source: an in-play instance to
/// its card (#539); a location's own forced ability to its map node (the Attic's
/// horror, #553); a Fast hand event by code — every copy (#539); a board-wide
/// effect to the act or agenda card when its code matches the current one, else no
/// card home (#540/#553/#556). Shared by [`build_resolution_options`] and the
/// forced-ack path.
pub(super) fn candidate_anchor(
    cand: &ResolutionCandidate,
    current_act: Option<&CardCode>,
    current_agenda: Option<&CardCode>,
) -> crate::engine::OptionTarget {
    use crate::engine::OptionTarget;
    match cand.source {
        CandidateSource::Hand => OptionTarget::HandCardByCode {
            investigator: cand.controller,
            code: cand.code.clone(),
        },
        CandidateSource::InPlay(instance_id) => OptionTarget::CardInstance(instance_id),
        CandidateSource::Location(location_id) => OptionTarget::Location(location_id),
        CandidateSource::Board => {
            if current_act == Some(&cand.code) {
                OptionTarget::Act
            } else if current_agenda == Some(&cand.code) {
                OptionTarget::Agenda
            } else {
                OptionTarget::Global
            }
        }
    }
}

/// Build the structured option list for a resolution frame: one
/// [`ChoiceOption`] per pending candidate, in `pending_triggers` order.
/// `OptionId(i)` is the index into the returned list — the Axis-A convention
/// shared with [`super::choice`]. The label distinguishes a hand Fast-event
/// play ([`CandidateSource::Hand`]) from an in-play reaction.
fn build_resolution_options(
    candidates: &[ResolutionCandidate],
    current_act: Option<&CardCode>,
    current_agenda: Option<&CardCode>,
) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, cand)| {
            let id = OptionId(u32::try_from(i).expect("option count fits in u32"));
            // Label distinguishes a hand Fast-event play from an in-play/board
            // reaction; the board anchor is the shared `candidate_anchor` (#553).
            let label = match cand.source {
                CandidateSource::Hand => format!("Play {} from hand", cand.code),
                CandidateSource::InPlay(_)
                | CandidateSource::Board
                | CandidateSource::Location(_) => {
                    format!("Resolve reaction: {}", cand.code)
                }
            };
            ChoiceOption::new(
                id,
                label,
                candidate_anchor(cand, current_act, current_agenda),
            )
        })
        .collect()
}

/// Re-run the reaction scan behind the open **reaction** window on top of the
/// stack and withdraw every candidate it no longer produces, emitting an
/// [`Event::ReactionOptionLapsed`] for each (#568). Called at both prompt sites,
/// so the option list a player sees is never older than the board.
///
/// # Why an offered option can stop being legal
///
/// The candidate list is a snapshot of one scan, and resolving one of its
/// options changes the board. The Rules Reference makes **initiation**, not
/// scanning, the moment that binds:
///
/// > A triggered ability can only be initiated if its effect has the potential
/// > to change the game state, and its cost (if any) has the potential to be
/// > paid in full, taking active cost modifiers into account.
///
/// Two Core-Set cases, both live today. Roland Banks 01001 (*"After you defeat
/// an enemy: Discover 1 clue at your location. (Limit once per round.)"*) and
/// Evidence! 01022 (*"Fast. Play after you defeat an enemy. Discover 1 clue at
/// your location."*) are offered together after a defeat; both are FAQ'd *"You
/// can only 'discover' a clue if there is a clue on your location."*, so Roland
/// taking the location's last clue leaves Evidence! nothing to do. And two copies
/// of Evidence! (cost 1) on a 1-resource wallet are both offered — playing either
/// empties the wallet the other would have to pay from.
///
/// # Why a re-scan rather than a re-check
///
/// [`scan_reactions_at`] *is* the definition of "may be offered here". Re-running
/// it and intersecting cannot drift from the gates the first scan applied, and
/// inherits any gate added later for free. The intersection
///
/// - **keeps multiplicity** — two copies of a card in hand are two candidates,
///   and one leaving hand withdraws exactly one of them;
/// - **never adds** — a card that entered play *during* the window was not in
///   play when the triggering condition occurred, so a fresh scan naming it is
///   not an invitation to offer it.
///
/// Two frames are deliberately skipped.
///
/// A [`FastWindow`](Continuation::FastWindow) has no reaction candidates by
/// construction ([`open_fast_window`] pushes an empty list) and no timing cell to
/// re-scan.
///
/// A **forced run** is skipped as *scope*, not because the rule spares it — it
/// does not: *"If a forced ability does not have the potential to change the game
/// state, the ability does not initiate"*, and *"The initiation of a forced
/// ability **that has the potential to change the game state** is mandatory each
/// time its specified timing point is met."* `collect_forced_hits` applies that
/// gate at collect time, so a 2+ lead-ordered run (#213) carries the same stale
/// verdict this function fixes for reactions. It is left alone here because
/// withdrawing from a *mandatory* run is a different shape — the run rejects
/// `Skip`, so an emptied one has to close itself rather than re-prompt — and
/// because no in-corpus forced effect charges a cost, which is what makes the
/// reaction case reachable harm. **TODO(#607):** re-validate the forced run once
/// that shape is decided.
///
/// Returns how many candidates were withdrawn, which only [`open_reaction_run`]
/// reads (as a debug-only tripwire).
fn withdraw_lapsed_candidates(cx: &mut Cx) -> usize {
    let Some((event, bucket)) = open_reaction_cell(cx.state) else {
        return 0;
    };
    let (event, bucket) = (event.clone(), bucket);
    let stored = cx
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .cloned()
        .unwrap_or_default();
    if stored.is_empty() {
        return 0;
    }

    let mut fresh = scan_reactions_at(cx.state, &event, bucket);
    let mut kept: Vec<ResolutionCandidate> = Vec::with_capacity(stored.len());
    let mut lapsed: Vec<ResolutionCandidate> = Vec::new();
    for candidate in stored {
        // Consume the match rather than just testing membership, so N stored
        // copies survive only as long as N fresh ones do.
        if let Some(pos) = fresh.iter().position(|f| *f == candidate) {
            fresh.remove(pos);
            kept.push(candidate);
        } else {
            lapsed.push(candidate);
        }
    }
    if lapsed.is_empty() {
        return 0;
    }
    for candidate in &lapsed {
        cx.events.push(Event::ReactionOptionLapsed {
            investigator: candidate.controller,
            code: candidate.code.clone(),
            reason: lapse_reason(cx.state, candidate),
        });
    }
    *cx.state
        .continuations
        .last_mut()
        .and_then(Continuation::pending_candidates_mut)
        .expect("withdraw_lapsed_candidates: the frame matched above is still on top") = kept;
    lapsed.len()
}

/// Withdraw **every** remaining candidate from an open `when`-cell window whose
/// triggering condition has just been prevented from resolving (#714).
///
/// Unlike [`withdraw_lapsed_candidates`], this is not a re-scan: the withdrawn
/// options are still perfectly initiable, and would still be found by a fresh
/// scan. What has gone is the condition they reference. The rule and its
/// citations are on `coordinator::prevented_in_the_when_cell`, which reads the
/// same signal one frame down; the half that matters here is
/// that Dodge 01023's ruling covers a `when`-cell ability, so the suppression
/// reaches the rest of the *current* cell and not only the cells after it.
///
/// Scoped twice over. To the `when` cell, because it is the only cell whose
/// abilities resolve before the condition does, so it is the only one a live
/// prevention signal can belong to. And to a **coordinator-owned** condition,
/// because a caller-owned one has already mutated the board and never walks its
/// `when` cell at all. #704 migrated the enemy attack into the coordinator, and
/// it inherited this rather than reimplementing it — the order #714 and #704
/// were sequenced in.
///
/// Both window modes are covered — a forced run empties the same way and closes
/// itself, rather than demanding a pick for a condition that is no longer
/// happening. Reachable since #704 gave the enemy attack a
/// [`ForcedTriggerPoint`] of its own — Dodge 01023's ruling is stated about a
/// **Forced** ability, and `crates/cards/tests/dodge.rs` proves it against
/// Silver Twilight Acolyte 01102. `0` for any other top frame, so the callers
/// need no guard.
///
/// [`ForcedTriggerPoint`]: super::forced_triggers::ForcedTriggerPoint
fn withdraw_suppressed_candidates(cx: &mut Cx) -> usize {
    if !cx.state.pending_cancellation {
        return 0;
    }
    let suppressed = match cx.state.continuations.last() {
        Some(Continuation::TimingPointWindow {
            event,
            bucket: EventTiming::When,
            candidates,
            ..
        }) if matches!(
            event.condition_resolution(),
            super::emit::ConditionResolution::Coordinator(_)
        ) =>
        {
            candidates.clone()
        }
        _ => return 0,
    };
    if suppressed.is_empty() {
        return 0;
    }
    for candidate in &suppressed {
        cx.events.push(Event::ReactionOptionLapsed {
            investigator: candidate.controller,
            code: candidate.code.clone(),
            reason: LapseReason::ConditionPrevented,
        });
    }
    cx.state
        .continuations
        .last_mut()
        .and_then(Continuation::pending_candidates_mut)
        .expect("withdraw_suppressed_candidates: the frame matched above is still on top")
        .clear();
    suppressed.len()
}

/// The `(event, cell)` an open **reaction** window on top of the stack was
/// scanned at — the question a re-scan has to re-ask, and the single place the
/// "which frames are re-validated" test lives (#568).
///
/// `None` for a forced run, for a [`FastWindow`](Continuation::FastWindow), and
/// for every non-window frame; the two callers turn that into their own no-op.
fn open_reaction_cell(state: &GameState) -> Option<(&TimingEvent, EventTiming)> {
    match state.continuations.last() {
        Some(Continuation::TimingPointWindow {
            event,
            bucket,
            mode: TimingMode::Reaction,
            ..
        }) => Some((event, *bucket)),
        _ => None,
    }
}

/// Best-effort attribution for a withdrawn candidate, for the client log
/// ([`LapseReason`], #568). The withdrawal has already been decided by the
/// re-scan in [`withdraw_lapsed_candidates`]; this only names a likely gate, so a
/// mislabel is cosmetic. Probes run most-specific first, and
/// [`LapseReason::NoLongerEligible`] is the honest residual when none matches.
fn lapse_reason(state: &GameState, candidate: &ResolutionCandidate) -> LapseReason {
    if !candidate_source_present(state, candidate) {
        return LapseReason::SourceGone;
    }
    if candidate.source == CandidateSource::Hand
        && check_play_resource_cost_payable(state, candidate.controller, &candidate.code).is_err()
    {
        return LapseReason::CostUnpayable;
    }
    let still_eligible = card_registry::current()
        .and_then(|reg| (reg.abilities_for)(&candidate.code))
        .and_then(|abilities| abilities.get(usize::from(candidate.ability_index)).cloned())
        .is_some_and(|ability| {
            ability_eligible(state, &ability, candidate.source, candidate.controller)
        });
    if still_eligible {
        LapseReason::NoLongerEligible
    } else {
        LapseReason::NoStateChange
    }
}

/// Whether a withdrawn candidate's card is still where the scan found it — the
/// [`LapseReason::SourceGone`] probe. Mirrors the zone each scan walks: hand for
/// a Fast play, the controller's card instances for an in-play reaction, the
/// current act/agenda for a board reaction.
fn candidate_source_present(state: &GameState, candidate: &ResolutionCandidate) -> bool {
    match candidate.source {
        CandidateSource::Hand => state
            .investigators
            .get(&candidate.controller)
            .is_some_and(|inv| inv.hand.contains(&candidate.code)),
        CandidateSource::InPlay(instance_id) => state
            .investigators
            .get(&candidate.controller)
            .is_some_and(|inv| {
                inv.controlled_card_instances()
                    .any(|card| card.instance_id == instance_id)
            }),
        CandidateSource::Board => {
            current_act_code(state).as_ref() == Some(&candidate.code)
                || current_agenda_code(state).as_ref() == Some(&candidate.code)
        }
        // No reaction scan produces a location-sourced candidate today (those are
        // forced location abilities — the Attic's horror, #553). Present iff the
        // location is still on the map, so a future location reaction attributes
        // sensibly rather than falling through to the residual.
        CandidateSource::Location(location_id) => state.locations.contains_key(&location_id),
    }
}

/// Whether `candidate` still survives a fresh scan of the open reaction window's
/// own timing cell — the single-candidate form of
/// [`withdraw_lapsed_candidates`], used as the fire-time gate in
/// [`fire_pending_trigger`] (#568).
///
/// Membership, not multiplicity: the question is "may *this* option still be
/// initiated", and one surviving match answers it. `true` for any other top
/// frame — a forced run is never withdrawn, and a [`FastWindow`] carries no
/// reaction candidates to re-scan.
///
/// [`FastWindow`]: Continuation::FastWindow
fn candidate_still_offerable(state: &GameState, candidate: &ResolutionCandidate) -> bool {
    let Some((event, bucket)) = open_reaction_cell(state) else {
        return true;
    };
    scan_reactions_at(state, event, bucket).contains(candidate)
}

/// Return [`AwaitingInput`] for the reaction window / forced run
/// [`push_reaction_window`] (via [`open_reaction_run`] /
/// [`open_forced_resolution`]) has just pushed as the top frame.
pub(crate) fn open_queued_reaction_window(cx: &mut Cx) -> EngineOutcome {
    // The queue and this prompt are not the same instant: `queue_event` queues the
    // window and then the point's *forced* abilities above it, and the `drive`
    // loop resolves those before this window is reached (ADR 0003) — the combat
    // callers additionally park the attack loop beneath it. Anything those steps
    // changed can have withdrawn an option already in the list (#568).
    withdraw_lapsed_candidates(cx);
    if cx
        .state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .is_some_and(Vec::is_empty)
    {
        // Nothing survived to offer. Close exactly as a `Skip` would — prompting
        // with an empty option list would strand the client.
        return close_reaction_window(cx);
    }
    let window = cx
        .state
        .continuations
        .last()
        .filter(|c| c.pending_candidates().is_some())
        .expect("open_queued_reaction_window: top frame is the just-queued window");
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip; the lead orders them)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let current_act = current_act_code(cx.state);
    let current_agenda = current_agenda_code(cx.state);
    let options = build_resolution_options(
        window
            .pending_candidates()
            .expect("open_queued_reaction_window: top window has candidates"),
        current_act.as_ref(),
        current_agenda.as_ref(),
    );
    let mut request = InputRequest::pick_single(
        format!(
            "Resolution window: {} option(s). \
             Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
            options.len(),
        ),
        options,
    );
    if !window.is_forced() {
        request = request.skippable();
    }
    EngineOutcome::AwaitingInput {
        request,
        // No multi-window state to disambiguate — routing keys off
        // the top of `state.open_windows`. Conventional 0 like the
        // commit-window's resume token.
        resume_token: ResumeToken(0),
    }
}

/// Resume an open reaction window with the player's response.
///
/// - [`InputResponse::PickSingle(OptionId(i))`]: fires the i-th pending
///   trigger via the evaluator. After firing, removes the entry. If pending
///   triggers remain, re-emits [`AwaitingInput`]; else closes the
///   window.
/// - [`InputResponse::Skip`]: closes the window provided no forced
///   triggers remain. Rejects when forced triggers are still pending.
/// - Other variants reject; the window stays open.
///
/// Closing the window pops the top entry from
/// [`GameState::open_windows`] and returns [`Done`].
pub(super) fn resume_reaction_window(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        // `OptionId(i)` indexes the single `pending_triggers` list (see
        // `build_resolution_options`); `fire_pending_trigger` dispatches on
        // the candidate's source (in-play ability vs. Axis-C hand play).
        InputResponse::PickSingle(OptionId(i)) => fire_pending_trigger(cx, *i),
        InputResponse::Skip => {
            // The window being skipped is the top frame (the prompt). Forced
            // abilities are mandatory — the forced run cannot be skipped
            // (RR p.2 / #213). The lead must pick one.
            if cx
                .state
                .continuations
                .last()
                .is_some_and(Continuation::is_forced)
            {
                return EngineOutcome::Rejected {
                    reason: "ResolveInput::Skip: forced abilities are mandatory; submit \
                             InputResponse::PickSingle(OptionId) to resolve one (the lead \
                             orders them)"
                        .into(),
                };
            }
            close_reaction_window(cx)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: reaction window expects InputResponse::PickSingle(OptionId) \
                 or InputResponse::Skip, got {other:?}",
            )
            .into(),
        },
    }
}

/// Fire the pending trigger at index `i` in the open reaction window.
/// Rejects out-of-bounds; the window stays open so the client can
/// retry with a corrected index.
// Mostly invariant-violation `unreachable!` arms + the Resolution-frame
// unwrapping (Axis-B T3); over the line limit but cohesive.
#[allow(clippy::too_many_lines)]
fn fire_pending_trigger(cx: &mut Cx, i: u32) -> EngineOutcome {
    // The window being driven is the top frame — the prompt the player is
    // responding to. Operate on it directly; the stack-is-resolution-order
    // invariant means the active window is always `last()` (Slice C-plumbing).
    // Snapshot to avoid borrowing state across the apply_effect call.
    let (trigger, pending_idx) = {
        let candidates = cx
            .state
            .continuations
            .last()
            .and_then(Continuation::pending_candidates)
            .expect("fire_pending_trigger: top frame is an open window/run");
        let idx = match usize::try_from(i) {
            Ok(idx) if idx < candidates.len() => idx,
            _ => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: reaction-window PickSingle(OptionId({i})) out of bounds \
                         (pending size {})",
                        candidates.len(),
                    )
                    .into(),
                };
            }
        };
        (candidates[idx].clone(), idx)
    };

    // The initiation gate, at initiation (#568). Both prompt sites withdraw
    // lapsed options before offering the list, so a pick taken from the prompt
    // the engine last emitted always passes this; what it rejects is a *stale*
    // pick — an option id replayed from an earlier prompt, or one fired by a
    // future path that reaches here without re-prompting. Rejecting is what keeps
    // `play_fast_event`'s `pay_play_cost` from saturating a cost it cannot pay.
    if !candidate_still_offerable(cx.state, &trigger) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: reaction-window PickSingle(OptionId({i})) names {code}, which can \
                 no longer be initiated (Rules Reference: a triggered ability can only be \
                 initiated if its effect has the potential to change the game state, and its cost \
                 can be paid in full). Re-read the current option list.",
                code = trigger.code,
            )
            .into(),
        };
    }

    // Axis C (#335): a hand candidate is *played*, not fired in place. Remove
    // it from the run first (so a suspending play resumes the remaining
    // siblings, not this one again — mirrors the in-play path below), then
    // play it.
    if trigger.source == CandidateSource::Hand {
        cx.state
            .continuations
            .last_mut()
            .and_then(Continuation::pending_candidates_mut)
            .expect("fire_pending_trigger: top frame is an open window/run")
            .remove(pending_idx);
        return play_fast_event(cx, &trigger);
    }

    // Look up the ability fresh from the registry. The card may have
    // changed state between scan and fire (exhausted, used, …) but
    // its ability list is static, so registry lookup is sufficient.
    let Some(reg) = card_registry::current() else {
        unreachable!(
            "fire_pending_trigger: registry was installed at scan time but is now \
             missing; the OnceLock contract guarantees once-set-stays-set"
        );
    };
    // Abilities resolve by code (works for in-play instances and scenario
    // board cards alike); `source` is the firing instance, when any.
    let code = trigger.code.clone();
    let abilities = (reg.abilities_for)(&code).unwrap_or_else(|| {
        unreachable!(
            "fire_pending_trigger: registry lost abilities for card {code:?} between \
             scan and fire; the OnceLock contract guarantees stable lookups",
        )
    });
    let ability = abilities
        .get(usize::from(trigger.ability_index))
        .unwrap_or_else(|| {
            unreachable!(
                "fire_pending_trigger: ability_index {idx} out of range for card {code:?} \
                 with {n} abilities; state-corruption invariant violation",
                idx = trigger.ability_index,
                n = abilities.len(),
            )
        })
        .clone();

    // Thread the source instance (if any) into the EvalContext so effects
    // that self-reference (`DiscardSelf`) or push source-attributed state
    // resolve against the firing card. Board-card candidates (act / agenda)
    // have no source; hand candidates were handled above.
    let mut eval_ctx = EvalContext::for_controller_with_optional_source(
        trigger.controller,
        trigger.source.instance(),
    );
    // For a `DamageAssigned` window whose source is an enemy attack, bind the
    // attacking enemy into the context so Guard Dog's native retaliate
    // (`Effect::Native("01021:retaliate")`) can name the attacker via
    // `eval_ctx.attacking_enemy`. Mirrors `failed_by` /
    // `clue_discovery_count`. `None` for all other window kinds. (C5b
    // #237.)
    match cx
        .state
        .continuations
        .last()
        .and_then(Continuation::window_timing_event)
    {
        Some(crate::engine::TimingEvent::DamageAssigned {
            source: crate::state::DamageSource::EnemyAttack { enemy },
            ..
        }) => {
            eval_ctx.set_attacking_enemy(*enemy);
        }
        // For `DiscoverClues`, bind the would-be discovery count so the
        // replacement effect (Cover Up's "discard that many") discards the
        // right number. Mirrors `attacking_enemy`. `count` is the **capped**
        // count — `discover_clue` caps at the location's clues before emitting
        // (#471) — so "that many" is what would actually have been discovered,
        // not what was requested.
        Some(crate::engine::TimingEvent::DiscoverClues { count, .. }) => {
            eval_ctx.set_clue_discovery_count(*count);
        }
        _ => {}
    }
    let usage_limit = ability.usage_limit;

    // Drop the fired entry *before* resolving its effect: if the effect
    // suspends (a forced ability that initiates a skill test — Frozen in
    // Fear 01164), the entry must already be consumed so the resume drives
    // the *remaining* siblings, not this one again. The window is still the
    // top frame here (apply_effect runs after).
    cx.state
        .continuations
        .last_mut()
        .and_then(Continuation::pending_candidates_mut)
        .expect("fire_pending_trigger: top frame is an open window/run")
        .remove(pending_idx);

    // Usage is consumed when the ability fires — the former "bump only on
    // `Done`" was purely defensive against an `unreachable!` `Rejected`. Bump
    // now, then push the effect for the drive loop; the window frame beneath
    // stays on top with its remaining candidates and `advance_resolution`
    // re-dispatches it once the effect (and any nested skill test) pops. In-scope
    // suspending forced effects (Frozen in Fear 01164) carry no usage limit, so
    // the early bump is a no-op for them. Slice D, #423.
    if usage_limit.is_some() {
        bump_usage_counter(cx.state, &trigger);
    }
    push_effect(cx, &ability.effect, eval_ctx);
    EngineOutcome::Done
}

/// Play the hand Fast-event `candidate` from the open resolution run (Axis C,
/// #335) — the [`CandidateSource::Hand`] resolution of [`fire_pending_trigger`].
/// Commences the play via the shared [`super::cards::commence_play`] (emit
/// [`crate::Event::CardPlayed`], leave hand — RR Appendix I step 3), then pushes a
/// [`Continuation::PlayFromHand`] frame **holding that card** (above the live
/// reaction window) and the `OnEvent` effect for the drive loop. On the effect's
/// completion, [`super::cards::dispose_play_from_hand`] places the event in
/// discard (RR Appendix I step 4) and the window beneath resumes its candidate
/// scan (Slice D #423).
///
/// This is the nesting site: the window may itself belong to an attack of
/// opportunity provoked by a *non-fast* play that is still mid-resolution
/// underneath. The frame stack keeps the two plays apart — this frame sits above
/// the outer play's and pops first (#604).
///
/// Pays the event's resource cost (RR p.22) via [`super::cards::pay_play_cost`]
/// before announcing the play, matching [`super::cards::play_card`] — a Fast
/// play skips the *action* cost, not the *resource* cost (#501). Affordability
/// and hand-presence were established by [`fire_pending_trigger`]'s fire-time
/// gate immediately above, not merely at scan time (#568), so the cost paid here
/// is a cost the wallet holds. The caller has already removed the candidate from
/// the run, so a suspending effect's resume drives the remaining siblings, not
/// this play again.
fn play_fast_event(cx: &mut Cx, candidate: &ResolutionCandidate) -> EngineOutcome {
    let controller = candidate.controller;
    // Find the event in the controller's hand by code (first match — copies
    // are fungible; resolving by code avoids stale indices after a prior play).
    // A miss is unreachable behind the fire-time gate, but it rejects rather than
    // panicking: nothing has been paid or moved yet, so the reject is clean, and
    // a panic here would take the whole session down over one bad option id
    // (#568).
    let Some(hand_idx) = cx
        .state
        .investigators
        .get(&controller)
        .and_then(|inv| inv.hand.iter().position(|c| *c == candidate.code))
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: {code} is no longer in {controller:?}'s hand and cannot be played \
                 from this window",
                code = candidate.code,
            )
            .into(),
        };
    };
    // Pay the resource cost before announcing the play (RR p.22): Fast plays
    // skip the action cost, not the resource cost. Affordability was re-checked
    // at fire time (#501, #568).
    super::cards::pay_play_cost(cx, controller, &candidate.code);
    let card = super::cards::commence_play(cx, controller, hand_idx);

    // Look up the matched OnEvent ability's effect from the registry.
    let reg = card_registry::current().unwrap_or_else(|| {
        unreachable!(
            "play_fast_event: registry installed at scan time is now missing; \
             the OnceLock contract guarantees once-set-stays-set"
        )
    });
    let abilities = (reg.abilities_for)(&candidate.code).unwrap_or_else(|| {
        unreachable!(
            "play_fast_event: registry lost abilities for {:?} between scan and play",
            candidate.code,
        )
    });
    let effect = abilities
        .get(usize::from(candidate.ability_index))
        .unwrap_or_else(|| {
            unreachable!(
                "play_fast_event: ability_index {} out of range for {:?}",
                candidate.ability_index, candidate.code,
            )
        })
        .effect
        .clone();
    let eval_ctx = EvalContext::for_controller(controller);

    // Push the event's disposal frame (above the window) holding the card, then
    // push its effect for the drive loop. On the effect's completion,
    // PlayFromHand disposal places the event in discard (RR Appendix I step 4)
    // and the window beneath resumes its candidate scan. (Slice D #423.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator: controller,
            card: Some(card),
        });
    push_effect(cx, &effect, eval_ctx);
    EngineOutcome::Done
}

/// Advance the resolution run on **top** of the stack after one of its
/// candidates resolved: withdraw any sibling the just-resolved candidate made
/// un-initiable ([`withdraw_lapsed_candidates`], #568), then close the run
/// (running its continuation) when none remain, else re-emit the pick prompt.
/// Called by the `drive` loop's window arm — the
/// window being driven is always the top frame (the stack-is-resolution-order
/// invariant), so there is no index to thread.
pub(super) fn advance_resolution(cx: &mut Cx) -> EngineOutcome {
    // The candidate that just resolved may have *prevented the condition itself*
    // — Cover Up 01007's replacement, Dodge 01023's cancel. Then no sibling of
    // this `when` cell may still be offered, however initiable it remains
    // (#714), and the window closes into a sequence the coordinator abandons.
    withdraw_suppressed_candidates(cx);
    // Short of that, the candidate may have withdrawn its siblings — spent
    // the shared wallet, or consumed what they would have acted on (#568). Re-ask
    // before re-prompting, so the list below is the *current* set of legal
    // initiations rather than the scan's.
    withdraw_lapsed_candidates(cx);
    let window = cx
        .state
        .continuations
        .last()
        .expect("advance_resolution: called with a window on top");
    let candidates = window
        .pending_candidates()
        .expect("advance_resolution: top frame is an open window/run");
    // Close when no candidate remains. Hand Fast-event plays (Axis C) ride
    // the candidate list alongside in-play triggers, so this single check
    // keeps a window with only a remaining hand play open.
    if candidates.is_empty() {
        return close_reaction_window(cx);
    }
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let current_act = current_act_code(cx.state);
    let current_agenda = current_agenda_code(cx.state);
    let options =
        build_resolution_options(candidates, current_act.as_ref(), current_agenda.as_ref());
    let mut request = InputRequest::pick_single(
        format!(
            "Resolution window: {} option(s). \
             Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
            options.len(),
        ),
        options,
    );
    if !window.is_forced() {
        request = request.skippable();
    }
    EngineOutcome::AwaitingInput {
        request,
        resume_token: ResumeToken(0),
    }
}

/// Bump the per-instance ability-usage counter for the just-fired
/// trigger. Called by [`fire_pending_trigger`] only for abilities
/// whose `usage_limit` is `Some(_)`; for abilities with no limit
/// nothing tracks them.
///
/// Routes on [`CandidateSource`]: `InPlay` bumps the `CardInPlay` instance —
/// the investigator card, a card in play, or a threat-area card, resolved by
/// instance id over all three zones (#448 cp3a folded the investigator card,
/// e.g. Roland Banks's seated `[reaction]`, onto this path; its usage now lives
/// on `investigator_card.ability_usage`). `Board`, `Hand`, and `Location`
/// candidates carry no per-instance usage limits and are `unreachable!` here.
///
/// **TODO (cancellation-counts-against-limit).** Rules Reference
/// page 14: *"If the effects of a card or ability with a limit or
/// maximum are canceled, it is still counted against the
/// limit/maximum, because the ability has been initiated."* Phase-3
/// has no cancellation primitive, so today we only bump on successful
/// resolution. When cancellation lands, the bump call must move
/// before the effect resolves (or fork into both paths) so canceled
/// fires still count.
fn bump_usage_counter(state: &mut GameState, trigger: &ResolutionCandidate) {
    let current_round = state.round;
    match trigger.source {
        CandidateSource::InPlay(instance_id) => {
            let inv = state
                .investigators
                .get_mut(&trigger.controller)
                .unwrap_or_else(|| {
                    unreachable!(
                        "bump_usage_counter: controller {ctl:?} vanished while reaction window \
                         was open; state-corruption invariant violation",
                        ctl = trigger.controller,
                    )
                });
            // Search the investigator card first, then cards in play, then the
            // threat area — the same zones `controlled_card_instances()` scans,
            // so an investigator-card reaction (Roland Banks) resolves here.
            let card = std::iter::once(&mut inv.investigator_card)
                .chain(inv.cards_in_play.iter_mut())
                .chain(inv.threat_area.iter_mut())
                .find(|c| c.instance_id == instance_id)
                .unwrap_or_else(|| {
                    unreachable!(
                        "bump_usage_counter: instance {instance_id:?} vanished from controller \
                         {ctl:?}'s investigator card / cards_in_play / threat area while reaction \
                         window was open; state-corruption invariant violation",
                        ctl = trigger.controller,
                    )
                });
            card.bump_ability_usage(trigger.ability_index, current_round);
        }
        CandidateSource::Board | CandidateSource::Hand | CandidateSource::Location(_) => {
            unreachable!(
                "bump_usage_counter: a usage-limited candidate must be an in-play instance \
                 (board / hand / location candidates carry no per-instance usage limits); \
                 candidate {trigger:?}"
            )
        }
    }
}

/// Close the reaction window / forced run on **top** of the stack: pop it and
/// run its kind-specific continuation, then return its outcome.
///
/// The window being closed is always the top frame — the player is acting on
/// the prompt it emitted (the stack-is-resolution-order invariant), so this
/// `pop()`s rather than threading an index (Slice C-plumbing). On a `Done`
/// continuation the loop dispatches whatever frame the close exposed (a
/// mid-resolution `SkillTest`, an `EncounterCard`, a forced run, …).
pub(super) fn close_reaction_window(cx: &mut Cx) -> EngineOutcome {
    // Reaction windows are all-optional, so `Skip` always closes them. The
    // "forced abilities are mandatory" rule lives in the forced resolution
    // run (its frame is `window: None` — Axis-B T5b), not here.
    let removed = cx
        .state
        .continuations
        .pop()
        .expect("close_reaction_window: a window frame is on top");

    // A framework window runs its kind-specific continuation, keyed off its
    // `FastWindowKind` (e.g. MythosAfterDraws → mythos_phase_end), and that
    // continuation may itself suspend — so propagate the outcome.
    //
    // A **reaction** window runs nothing, because no triggering condition has a
    // continuation of its own any more: since #727 the last two that did — the
    // enemy attack and its soak window — walk the coordinator like everything
    // else, so what used to be post-window work is now a resolve step or a
    // parked frame (`DealDamage`, `AttackLoop`, the coordinator's own
    // `TimingPoint`). It returns `Done` and the `drive` loop dispatches whatever
    // frame the pop exposed. The forced run (#213/#434) never had one either,
    // which is why the two share an arm.
    let continuation = match &removed {
        Continuation::FastWindow { kind, .. } => run_fast_continuation(cx, *kind),
        _ => EngineOutcome::Done,
    };
    if matches!(continuation, EngineOutcome::AwaitingInput { .. }) {
        return continuation;
    }
    debug_assert!(
        matches!(continuation, EngineOutcome::Done),
        "close_reaction_window: window continuation returned unexpected {continuation:?} \
         (expected Done or AwaitingInput)",
    );

    // The window is closed and its continuation ran to `Done`. Return to the
    // `drive` loop, which dispatches whatever frame is now top — a `SkillTest`
    // mid-resolution (its driver picks up the remaining steps), an `EncounterCard`
    // to dispose, a forced run, or idle. No reach-down into `skill_test::advance`
    // (Slice C-plumbing).
    EngineOutcome::Done
}

/// Continuation when a framework **fast** window ([`Continuation::FastWindow`])
/// closes, keyed on its [`FastWindowKind`]. Called from the auto-skip path in
/// [`open_fast_window`] and from [`close_reaction_window`].
///
/// A phase window routes to the `*Phase` anchor beneath it (slice 1a, #393): the
/// anchor's `resume` — not the [`PhaseStep`] — selects the relocated body (the
/// Mythos/Investigation transitions, the Enemy attack-loop step, the Upkeep
/// 4.2–4.6 cascade). A skill-test window (#374) re-enters the skill-test driver;
/// its cursor was pre-advanced before the window opened. Returns `Done` or
/// `AwaitingInput` when a body suspends.
pub(super) fn run_fast_continuation(cx: &mut Cx, kind: FastWindowKind) -> EngineOutcome {
    // This is the window's *own* continuation, run inline on close — including
    // the open-time auto-skip path in `open_fast_window`, which relies on it
    // advancing the phase / skill-test driver **synchronously** to reach the next
    // suspending step (the commit prompt, the next phase window). It is not a
    // driver-to-driver reach-down, so it stays imperative (the genuine reach-down
    // — the redundant `skill_test::advance` *after* this in `close_reaction_window`
    // — was removed in Slice C-plumbing).
    match kind {
        FastWindowKind::Phase(_) => super::phases::anchor_on_child_pop(cx),
        FastWindowKind::SkillTest { .. } => super::skill_test::advance(cx),
    }
}

/// Advance the enemy-phase cursor past `investigator` and open the next
/// window (C5b #237).
///
/// Since #704 there is one caller: `combat::finish_attack_loop`, run when the
/// attack loop's list drains. The [`PhaseStep::BeforeInvestigatorAttacked`]
/// continuation no longer calls it — the loop only *queues* its attacks, so a
/// `Done` there means queued, not finished (ADR 0003). Advances the
/// `EnemyPhase` anchor's `attacking`
/// cursor to the next Active investigator AFTER `investigator` via
/// [`cursor::next_active_investigator_after`](super::cursor::next_active_investigator_after)
/// — the helper indexes off `turn_order` (not the filtered-Active
/// list), so `investigator` itself can have been defeated mid-loop and
/// the right successor is still found. Then opens
/// [`PhaseStep::BeforeInvestigatorAttacked`] again if the cursor
/// advanced to `Some`, otherwise [`PhaseStep::AfterAllInvestigatorsAttacked`].
pub(super) fn after_enemy_phase_attacks(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let next = super::cursor::next_active_investigator_after(cx.state, investigator);
    super::phases::open_attack_window(cx, next)
}

/// Open a printed Fast-play window of the given kind. Then either:
///
/// - Pushes the [`FastWindow`](crate::state::Continuation::FastWindow) onto the
///   continuation stack if any pending reaction triggers or Fast-eligible plays
///   are detected. The
///   apply loop's existing "pending reactions → `AwaitingInput`" path
///   then surfaces the wait at the dispatch tail.
/// - Or closes the window immediately, pops the transiently
///   pushed window, and runs [`run_fast_continuation`] inline. This
///   **auto-skip** path saves a UI round-trip when nobody can act.
///
/// # Push-then-scan ordering
///
/// The window is pushed onto [`GameState::open_windows`] **before**
/// [`any_fast_play_eligible`] is called. This is load-bearing:
/// [`check_play_card`]'s timing gate reads
/// `state.open_windows.last()` to decide whether a Fast card is
/// eligible (`permissive_window`). If the window weren't on the stack
/// yet, any Fast event held during the Mythos phase would be evaluated
/// as ineligible (`active_during_investigation = false`,
/// `permissive_window = false`) and the window would auto-skip even
/// though Fast plays are available.
///
/// On the auto-skip path the window is popped before returning so the
/// net effect on `state.open_windows` is identical to the pre-fix
/// behaviour (window never lands persistently on the stack).
///
/// Returns the continuation's outcome on the auto-skip path (today always
/// [`EngineOutcome::Done`]; propagates [`EngineOutcome::AwaitingInput`] once
/// #111 step 4.5 can suspend); returns [`EngineOutcome::Done`] immediately on
/// the wait path (window left on the stack).
pub(super) fn open_fast_window(cx: &mut Cx, kind: FastWindowKind) -> EngineOutcome {
    // Push first so any_fast_play_eligible's check_play_card call sees
    // this window in state.open_windows when evaluating permissive_window.
    // Framework windows are `FastWindow` (#433 A-ii); the `FastWindowKind`
    // discriminant reproduces `kind` and routes the
    // close continuation. Fast windows carry no reaction candidates — they are
    // pure Fast-gates (no `TimingEvent` reaction matches a framework window), so
    // the candidate list is always empty; the Fast-play opportunity is gated by
    // `any_fast_play_eligible` below.
    let candidates = Vec::new();
    cx.state.continuations.push(Continuation::FastWindow {
        candidates,
        fast_actors: FastActorScope::Any,
        kind,
    });

    let has_pending = !cx
        .state
        .top_window()
        .expect("just pushed; cannot be empty")
        .pending_candidates()
        .expect("top_window is an open window/run")
        .is_empty();
    let has_fast_eligible = any_fast_play_eligible(cx.state);

    if !has_pending && !has_fast_eligible {
        // Auto-skip: nothing to do. Pop the window we just pushed and run the
        // continuation inline, so the net effect on the continuation stack is
        // the same as before.
        let _ = cx.state.continuations.pop();
        return run_fast_continuation(cx, kind);
    }
    // Otherwise the window stays on the stack. The guard at the top of
    // apply() and resume_reaction_window / resolve_input handle the
    // wait + close path.
    EngineOutcome::Done
}

/// Pure-validation peer to [`play_card`]. Returns `Ok` if the named
/// card is currently playable by `investigator`, `Err(reason)` if
/// not. The check is the existing `play_card` validation block lifted
/// verbatim — no behavior change at `play_card`'s call site.
///
/// Used by [`play_card`] (which then runs the mutation block on the
/// `Ok` payload) and by `any_fast_play_eligible` (which only
/// inspects `Ok` vs `Err`).
/// RR p.11 (#495): an event card may be played only if its `OnPlay` effect has
/// the potential to change the game state right now (Working a Hunch 01037 at a
/// 0-clue location is unplayable). Events only — assets and other card types
/// always change state by entering play. Uses the same conservative
/// `effect_can_change_state` evaluator as the reaction/forced initiation gates,
/// so only provable no-ops are blocked. `Ok(())` when playable.
fn check_event_play_changes_state(
    state: &GameState,
    investigator: InvestigatorId,
    code: &CardCode,
    card_type: CardType,
    abilities: &[Ability],
) -> Result<(), Cow<'static, str>> {
    if card_type != CardType::Event {
        return Ok(());
    }
    let ctx = EvalContext::for_controller_with_optional_source(investigator, None);
    let changes_state = abilities.iter().any(|a| {
        matches!(a.trigger, Trigger::OnPlay)
            && crate::engine::evaluator::effect_can_change_state(state, ctx, &a.effect)
    });
    if changes_state {
        Ok(())
    } else {
        Err(format!(
            "PlayCard: {code}'s effect cannot change the game state right now, so it \
             cannot be played (RR p.11)."
        )
        .into())
    }
}

/// Gates RR p.19 slot capacity: Assets only; the only hard slot reject — a merely-full
/// slot is not rejected here, make-room at enter-play handles it.
fn check_play_slot_satisfiable(
    card_type: CardType,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    if card_type != CardType::Asset {
        return Ok(());
    }
    if let Some(slot) = super::slots::unsatisfiable_slot(code) {
        return Err(format!(
            "PlayCard: {code} needs more {slot:?} slots than the investigator has \
             (slot capacity exceeded; RR p.19)."
        )
        .into());
    }
    Ok(())
}

pub(crate) fn check_play_card(
    state: &GameState,
    investigator: InvestigatorId,
    hand_index: u8,
) -> Result<super::PlayCheckResult, Cow<'static, str>> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(format!("PlayCard: investigator {investigator:?} is not in state").into());
    };
    if inv.status != Status::Active {
        return Err(format!(
            "PlayCard: {investigator:?} is not Active (status {:?})",
            inv.status,
        )
        .into());
    }
    let idx = usize::from(hand_index);
    if idx >= inv.hand.len() {
        return Err(format!(
            "PlayCard: hand_index {hand_index} out of bounds (hand size {})",
            inv.hand.len(),
        )
        .into());
    }
    let code: CardCode = inv.hand[idx].clone();
    // Resolve card type and abilities (also yields is_fast + card_type) before
    // applying the phase/active-investigator gate so the gate can branch on
    // is_fast AND card_type per the Rules Reference (p. 11).
    // Invariant: `resolve_play_target` currently returns only `Ok(...)` (success)
    // or `Err(EngineOutcome::Rejected { ... })` (validation failure). If a future
    // PR extends it to return `AwaitingInput` (e.g. for a card requiring in-
    // validation target selection), this `unreachable!()` will panic; the
    // validator's caller chain in `play_card` would need to be redesigned to
    // thread the `AwaitingInput` outcome back through `check_play_card`'s
    // `Result` shape. Pinning the invariant loudly here is intentional —
    // silent `AwaitingInput` propagation through a `Result<_, Cow>` would
    // produce wrong gameplay.
    // The destination is re-derived from the code at disposal time
    // (`dispose_play_from_hand`), not carried through validation — commencing a
    // play is destination-agnostic (#604).
    let (_destination, abilities, is_fast, card_type) =
        match super::cards::resolve_play_target(&code) {
            Ok(v) => v,
            Err(EngineOutcome::Rejected { reason }) => return Err(reason),
            Err(other) => {
                unreachable!("resolve_play_target returned non-Rejected outcome: {other:?}")
            }
        };
    // Reaction-event gate (Axis C, #335 / #304): a Fast event whose play
    // instruction is a triggering condition is modeled as a `TriggerKind::Reaction`
    // `OnEvent` ability (e.g. Evidence! 01022's "Play after you defeat an enemy").
    // RR p.11: such an event "may be played any time its play instructions
    // specify" — i.e. ONLY in its matching reaction window, where Axis C offers
    // it as a `PickSingle` option (the window path runs `play_fast_event`,
    // bypassing this gate). It is never a free-timing standalone play, so reject
    // it from the `PlayCard` action — otherwise `play_card` would run only its
    // (absent) `OnPlay` abilities and silently discard it for no effect.
    //
    // Gate only on a **Reaction** `OnEvent`: an event that plays normally (an
    // `OnPlay` effect) but carries a **Forced** `OnEvent` for its *in-play*
    // form is not a reaction event (Barricade 01038 attaches on play, then its
    // attachment's Forced discards it on leave). Such an event is played as a
    // standard action.
    if card_type == CardType::Event
        && abilities.iter().any(|a| {
            matches!(
                a.trigger,
                Trigger::OnEvent {
                    kind: crate::dsl::TriggerKind::Reaction,
                    ..
                }
            )
        })
    {
        return Err(format!(
            "PlayCard: {code} is a reaction event — it may only be played in response \
             to its triggering condition (its reaction window), not as a standalone \
             action (RR p.11)."
        )
        .into());
    }
    // RR p.11 initiation gate (#495): an event can't be played if its OnPlay
    // effect can't change game state — open-turn menu OR Fast window route here.
    check_event_play_changes_state(state, investigator, &code, card_type, &abilities)?;
    // RR p.19 slots (#498): reject only when the card needs more of a slot type
    // than the investigator has capacity for — unsatisfiable even after discarding
    // every occupying asset. A merely-full slot is NOT rejected here; the play
    // proceeds and discards occupiers to make room at enter-play time. Unreachable
    // in the current corpus (max need is Hand×2 = cap 2); no silent no-op.
    check_play_slot_satisfiable(card_type, &code)?;
    // Timing gate — see play_card doc-comment "# Timing gate" section.
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let owner_is_active = state.active_investigator == Some(investigator);
    let permissive_window = state
        .top_window()
        .is_some_and(|w| w.permits_fast(investigator));
    // "Play only during your turn" (Mind over Matter 01036, Working a Hunch
    // 01037, …): a Fast card with this clause is restricted to the active
    // investigator's Investigation turn — never an out-of-turn permissive Fast
    // window (the Mythos `MythosAfterDraws` window). FAQ: "'your turn' is within
    // the Investigation phase."
    let only_during_turn = card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&code))
        .is_some_and(CardMetadata::play_only_during_turn);
    // Non-asset/non-event card types are filtered out by
    // `resolve_play_target` above, so `card_type` here is always one of
    // `Asset` or `Event`. The non-Fast arm collapses both into the
    // strict gate; the Fast arms split because Rules Reference p. 11
    // gives events and assets different scopes (any vs owner-only).
    let allowed = if is_fast {
        match card_type {
            CardType::Event => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || permissive_window
                }
            }
            CardType::Asset => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || (owner_is_active && permissive_window)
                }
            }
            // Unreachable: `resolve_play_target` rejects every other
            // `CardType` before we get here. Fall back to the strict
            // gate so a future relaxation of `resolve_play_target` does
            // not silently over-permit anything.
            _ => active_during_investigation,
        }
    } else {
        active_during_investigation
    };
    if !allowed {
        return Err(format!(
            "PlayCard: card not playable in this timing window. \
             Rules Reference p. 11: non-Fast cards require Investigation + active \
             investigator; Fast events require active investigator or a window whose \
             fast_actors permits the actor; Fast assets additionally require the OWNER \
             (active investigator) to act. \
             Got is_fast={is_fast}, card_type={card_type:?}, phase={phase:?}, \
             active={active:?}, actor={investigator:?}, owner_is_active={owner_is_active}, \
             permissive_window={permissive_window}.",
            phase = state.phase,
            active = state.active_investigator,
        )
        .into());
    }
    // Playing a card is an action (RR p.5), so a non-fast play needs an action
    // point (validate-first; `play_card` spends it). Fast plays are not actions.
    check_play_action_available(state, investigator, is_fast, &code)?;
    // Playing a card is paying its cost (RR p.22, Initiation Sequence): the
    // resource cost must be established as payable before initiation. Both Fast
    // and non-Fast plays pay it — Fast only skips the *action* cost (#501).
    check_play_resource_cost_payable(state, investigator, &code)?;
    Ok(super::PlayCheckResult {
        abilities,
        is_fast,
        card_type,
    })
}

/// A non-fast play is an action (RR p.5) and needs an action point; fast plays
/// are not actions and have no such cost (#378). Returns the reject reason when
/// a non-fast play has no action available.
fn check_play_action_available(
    state: &GameState,
    investigator: InvestigatorId,
    is_fast: bool,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    if is_fast {
        return Ok(());
    }
    let remaining = state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.actions_remaining);
    if remaining < 1 {
        return Err(format!(
            "PlayCard: playing {code} is an action and requires 1 action point; \
             {investigator:?} has {remaining}"
        )
        .into());
    }
    Ok(())
}

/// Playing a card is paying its resource cost in full (RR p.22, Initiation
/// Sequence — the cost must be established as payable before initiation, and is
/// then paid before attacks of opportunity resolve). Returns the reject reason
/// when `investigator` cannot pay `code`'s printed cost. A 0-cost card is always
/// affordable.
///
/// Cards with no fixed printed cost (`play_cost()` is `None` — an X-cost card,
/// or a permanent) are **not yet modeled** — rejected loudly rather than
/// silently played for free. No implemented card hits this: such cards are
/// unplayable stubs refused earlier by `resolve_play_target`, and permanents
/// enter play at setup rather than via `PlayCard`. The branch is currently
/// unreachable in the corpus; it guards a future implemented X-cost card
/// (deferral split from #501).
///
/// Short-circuits to `Ok` when the registry isn't installed — the metadata-free
/// validation paths the engine's own unit tests exercise; the real play path
/// always has a registry installed by the time it reaches here.
fn check_play_resource_cost_payable(
    state: &GameState,
    investigator: InvestigatorId,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    let Some(meta) = card_registry::current().and_then(|reg| (reg.metadata_for)(code)) else {
        return Ok(());
    };
    let resources = state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.resources);
    match meta.play_cost() {
        // Printed costs are non-negative; `try_from` clamps a (nonexistent)
        // negative cost to 0 = free, never a panic.
        Some(cost) => {
            let cost = u8::try_from(cost).unwrap_or(0);
            if resources < cost {
                return Err(format!(
                    "PlayCard: playing {code} costs {cost} resource(s); \
                     {investigator:?} has {resources}"
                )
                .into());
            }
            Ok(())
        }
        None => Err(format!(
            "PlayCard: {code} has no fixed printed cost (X-cost or permanent), \
             which is not yet modeled — deferred from #501."
        )
        .into()),
    }
}

/// Reject an activation whose effect needs a target it cannot get, at the check
/// layer (before any cost is paid) so the rejection is honest for
/// `any_fast_play_eligible` and `Effect::Fight` / `DealDamageToEnemy` can treat
/// a missing target as an invariant violation.
///
/// - **Fight:** an ability printing the **Fight** designator needs ≥1 enemy *at
///   your location* (0 = no target, rejected pre-cost; 2+ suspends to a
///   `PickSingle` target-pick in the evaluator). Keyed off the declared
///   designator rather than an `Effect::Fight` at the effect root (#696): the
///   designator is what makes the ability a fight action, and an effect-root
///   match misses a `Seq`-wrapped one.
///   Scope is co-located, not engaged-only: per RR you choose an enemy at your
///   location to attack and need not already be engaged (matches the basic
///   Fight action — an Aloof enemy, or one engaged with another investigator
///   in MP, is a legal weapon target). #451.
/// - **`DealDamageToEnemy`:** needs ≥1 enemy in the chosen scope (e.g. "at your
///   location"). ≥1 proceeds — 2+ suspends via the `Choose` resolver — so only
///   the empty case rejects here; this is why the effect is typed, not `Native`
///   (Beat Cop can't pay its discard-self cost for no legal target). `amount` is
///   not consulted (a degenerate `amount: 0` ability — none in scope — would
///   still require a target here even though its handler is a no-op).
/// - **`Investigate`:** needs the controller at a revealed location to test
///   (Flashlight can't pay its supply cost with nothing to investigate).
fn check_effect_target_available(
    state: &GameState,
    investigator: InvestigatorId,
    designator: Option<ActionDesignator>,
    effect: &crate::dsl::Effect,
) -> Result<(), Cow<'static, str>> {
    if designator == Some(ActionDesignator::Fight)
        && super::combat::enemies_in_scope(state, investigator, super::combat::fight_target_scope())
            .is_empty()
    {
        return Err(
            "ActivateAbility: a Fight ability needs an enemy at your location (none co-located)"
                .into(),
        );
    }
    if let crate::dsl::Effect::DealDamageToEnemy {
        target: crate::dsl::EnemyTarget::Chosen(choose),
        ..
    } = effect
    {
        if super::combat::enemies_in_scope(state, investigator, choose.scope).is_empty() {
            return Err(
                "ActivateAbility: a 'deal damage to an enemy at your location' ability \
                 needs at least one enemy at your location"
                    .into(),
            );
        }
    }
    if matches!(effect, crate::dsl::Effect::Investigate { .. }) {
        let revealed_here = state
            .investigators
            .get(&investigator)
            .and_then(|inv| inv.current_location)
            .and_then(|loc| state.locations.get(&loc))
            .is_some_and(|loc| loc.revealed);
        if !revealed_here {
            return Err(
                "ActivateAbility: an Investigate ability needs a revealed location to investigate"
                    .into(),
            );
        }
    }
    Ok(())
}

/// The RR initiation gate on the activation path (#639).
///
/// `data/rules-reference/rules/glossary/Ability.md`, "Triggered Abilities":
///
/// > A triggered ability can only be initiated if its effect has the potential
/// > to change the game state, and its cost (if any) has the potential to be
/// > paid in full, taking active cost modifiers into account.
///
/// and `glossary/Costs.md`: *"An ability cannot initiate – and therefore its
/// costs cannot be paid – if the resolution of its effect will not change the
/// game state."* Rejecting here rather than during resolution is what keeps the
/// action point and the ability's costs unspent.
///
/// Uses the same conservative
/// [`effect_can_change_state`](crate::engine::evaluator::effect_can_change_state)
/// evaluator as the play, reaction, and forced-trigger gates, so only provable
/// no-ops are blocked. Being part of [`check_activate_ability`] rather than
/// [`activate_ability`] is what keeps the turn menu and the fast-window
/// enumerator — both of which filter on this validator — from offering an
/// activation that would reject.
fn check_activation_changes_state(
    state: &GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
    code: &CardCode,
    effect: &crate::dsl::Effect,
) -> Result<(), Cow<'static, str>> {
    let ctx = EvalContext::for_controller_with_optional_source(investigator, source.instance());
    if crate::engine::evaluator::effect_can_change_state(state, ctx, effect) {
        return Ok(());
    }
    Err(format!(
        "ActivateAbility: {code}'s effect cannot change the game state right now, so the \
         ability cannot be initiated (RR \"Ability\"/\"Costs\")."
    )
    .into())
}

/// Reject an ability mixing [`Cost::DiscardSelf`](crate::dsl::Cost::DiscardSelf)
/// with another source-referencing cost: `DiscardSelf` removes the source, so it
/// must be the sole such cost (Beat Cop / Knife list only it). Deliberately
/// unlifted until a card needs the combo — no tracking issue on purpose (YAGNI);
/// whoever hits this rejection files one.
fn reject_incompatible_costs(costs: &[crate::dsl::Cost]) -> Result<(), Cow<'static, str>> {
    use crate::dsl::Cost;
    if costs.iter().any(|c| matches!(c, Cost::DiscardSelf))
        && costs
            .iter()
            .any(|c| matches!(c, Cost::Exhaust | Cost::SpendUses { .. }))
    {
        return Err(
            "ActivateAbility: Cost::DiscardSelf cannot combine with Exhaust/SpendUses on the \
             same ability (it removes the source); lift if a card ever needs the combo"
                .into(),
        );
    }
    Ok(())
}

/// Reject an ability whose *"Limit X per \[period\]"* sits on a source with no
/// card instance to record the use against — a location, an enemy, the act or
/// the agenda.
///
/// Usage state is `CardInPlay::ability_usage`, a per-instance map, and a
/// location has no instance (`bump_usage_counter`'s `unreachable!` says so for
/// the reaction path). Making these sources activatable is what first puts that
/// branch behind player input, and **a panic reachable from player input must
/// not ship** — so the limit is refused, loudly, rather than silently ignored
/// or crashed into.
///
/// No Core or Dunwich card in the corpus reaches this: the Parlor 01115's
/// Resign is unlimited. Dunwich prints two that will — Base of the Hill 02282
/// (*"\[action\]: **Investigate.** … (Limit once per round.)"*) and Ten-Acre
/// Meadow 02246 (*"(Group limit once per game)"*) — and **#699** builds the
/// capability they need. `glossary/Limits_and_Maximums.md` makes the key scoped
/// rather than global there, verbatim: *"Unless stated otherwise, limits are
/// player specific"*, while a group limit *"applies to the entire group of
/// investigators"*.
fn reject_untrackable_usage_limit(
    source: AbilitySource,
    code: &CardCode,
    usage_limit: Option<crate::dsl::UsageLimit>,
) -> Result<(), Cow<'static, str>> {
    if usage_limit.is_none() || source.instance().is_some() {
        return Ok(());
    }
    Err(format!(
        "ActivateAbility: {code}'s ability carries a usage limit, but {source:?} has no card \
         instance to record uses against (usage state lives on in-play instances); \
         TODO(#699): usage limits on a location / act / agenda need a state-level counter"
    )
    .into())
}

/// Reject a cost that has to be paid *by the source* when the source has no card
/// instance to pay it with — `Exhaust`, `SpendUses` and `DiscardSelf` on a
/// location or an enemy.
///
/// Refused at validation rather than at payment so the turn menu never offers
/// it: `pay_activation_costs` addresses these costs through a `CardInPlay`
/// (#706), and a location has none. No corpus card needs one — the Parley
/// abilities cost cards, clues or resources (Herman Collins 01138 *"Choose and
/// discard 4 cards from your hand"*, Peter Warren 01139 *"Spend 2 clues"*,
/// Victoria Devereux 01140 and Mob Enforcer 01101 *"Spend … resources"*), all
/// investigator-side. Deliberately unlifted with no tracking issue (YAGNI, as
/// with [`reject_incompatible_costs`]); whoever prints such a card files one.
fn reject_source_costs_without_an_instance(
    source: AbilitySource,
    code: &CardCode,
    costs: &[crate::dsl::Cost],
) -> Result<(), Cow<'static, str>> {
    use crate::dsl::Cost;
    if source.instance().is_some() {
        return Ok(());
    }
    let Some(cost) = costs.iter().find(|c| {
        matches!(
            c,
            Cost::Exhaust | Cost::SpendUses { .. } | Cost::DiscardSelf
        )
    }) else {
        return Ok(());
    };
    Err(format!(
        "ActivateAbility: {code}'s {cost:?} cost must be paid by the source, but {source:?} has \
         no card instance to pay it with"
    )
    .into())
}

/// Pure-validation peer to [`activate_ability`]. Mirrors
/// [`check_play_card`]: validation block lifted verbatim, no behavior
/// change at the call site.
///
/// Returns `Ok(ActivateCheckResult)` if the ability is currently
/// activatable, `Err(reason)` otherwise. Does not mutate state.
pub(crate) fn check_activate_ability(
    state: &GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
    ability_index: u8,
) -> Result<super::ActivateCheckResult, Cow<'static, str>> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(
            format!("ActivateAbility: investigator {investigator:?} is not in state").into(),
        );
    };
    if inv.status != Status::Active {
        return Err(format!(
            "ActivateAbility: {investigator:?} is not Active (status {:?})",
            inv.status,
        )
        .into());
    }
    // Which sources exist is the reachability predicate's answer, and it is the
    // same one the turn-menu enumerator lists from (#707). Addressed by
    // identity, never by position: the position a validation computes is stale
    // the moment a cost removes the source (#706).
    let source_card = crate::engine::ability_source::resolve(state, investigator, source)?;
    let source_code = source_card.code().clone();
    let source_exhausted = source_card.exhausted();
    let source_uses = source_card.uses();

    // Invariant: `resolve_activated_ability` currently returns only `Ok(...)`
    // (success) or `Err(EngineOutcome::Rejected { ... })` (validation failure).
    // If a future PR extends it to return `AwaitingInput` (e.g. for an ability
    // requiring target selection during validation), this `unreachable!()` will
    // panic; the validator's caller chain in `activate_ability` would need to be
    // redesigned to thread the `AwaitingInput` outcome back through
    // `check_activate_ability`'s `Result` shape. Mirrors the same invariant
    // comment on `resolve_play_target` in `check_play_card`.
    let super::abilities::ActivatedAbility {
        action_cost,
        designator,
        costs,
        effect,
        usage_limit,
    } = match super::abilities::resolve_activated_ability(&source_code, ability_index) {
        Ok(v) => v,
        Err(EngineOutcome::Rejected { reason }) => return Err(reason),
        Err(other) => {
            unreachable!("resolve_activated_ability returned non-Rejected outcome: {other:?}")
        }
    };
    reject_untrackable_usage_limit(source, &source_code, usage_limit)?;

    // Gate: branch on action_cost now that we have it.
    // Fast abilities (action_cost == 0) may be used at any player window.
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let in_permissive_window = state
        .top_window()
        .is_some_and(|w| w.permits_fast(investigator));
    if action_cost > 0 {
        // Action-cost ability: requires Investigation phase + active investigator.
        if !active_during_investigation {
            return Err(format!(
                "ActivateAbility: action-cost ability requires Investigation phase + \
                 active investigator (phase was {:?}, active {:?})",
                state.phase, state.active_investigator,
            )
            .into());
        }
    } else {
        // Fast ability: active during Investigation OR permissive window.
        if !active_during_investigation && !in_permissive_window {
            return Err(
                "ActivateAbility: Fast ability requires either active investigator \
                         during Investigation, or an open window whose fast_actors permits \
                         this investigator"
                    .into(),
            );
        }
    }

    // Re-borrow inv after state borrows above.
    let inv = state.investigators.get(&investigator).expect("checked");

    // Action-economy check.
    if inv.actions_remaining < action_cost {
        return Err(format!(
            "ActivateAbility: needs {action_cost} action(s); investigator has {}",
            inv.actions_remaining,
        )
        .into());
    }

    // Validate every payment cost is payable. Done as a pure read
    // before any mutation so an all-or-nothing reject leaves state
    // untouched.
    for cost in &costs {
        if let Err(reason) =
            super::abilities::check_cost_payable(cost, inv, source_exhausted, &source_uses)
        {
            return Err(reason.into());
        }
    }

    reject_incompatible_costs(&costs)?;
    reject_source_costs_without_an_instance(source, &source_code, &costs)?;
    check_effect_target_available(state, investigator, designator, &effect)?;
    check_activation_changes_state(state, investigator, source, &source_code, &effect)?;

    Ok(super::ActivateCheckResult {
        source_code,
        action_cost,
        designator,
        costs,
        effect,
        source_exhausted,
    })
}

/// Returns `true` if any investigator has at least one playable Fast
/// option in the current state — either a Fast card in hand or a
/// non-exhausted 0-action Activated ability on a card in play.
/// Used by [`open_fast_window`] to short-circuit windows where nobody
/// can act.
///
/// Eligibility uses the extracted [`check_play_card`] /
/// [`check_activate_ability`] validators so the gate is exactly the
/// existing `PlayCard` / `ActivateAbility` gate — no parallel
/// implementation, no drift.
///
/// Returns `false` when the card registry isn't installed (tests
/// that don't touch card data) — same fallback as
/// [`scan_pending_triggers`].
pub(super) fn any_fast_play_eligible(state: &GameState) -> bool {
    !enumerate_fast_plays(state).is_empty()
}

/// Drive a framework Fast window that is on top of the stack (#476): surface the
/// currently-eligible fast plays as a **skippable** `PickSingle`, or close the
/// window (running its continuation) when none remain. Called by the `drive`
/// loop's `FastWindow` arm — both when the window first parks and each time it is
/// re-exposed after a fast play resolves (the re-open loop). The window stays on
/// top across the prompt; `resume_window` dispatches the pick, or closes on Skip.
pub(super) fn drive_fast_window(cx: &mut Cx) -> EngineOutcome {
    let plays = enumerate_fast_plays(cx.state);
    if plays.is_empty() {
        // Nothing (more) to play: close + run the window's continuation.
        return close_reaction_window(cx);
    }
    let options = plays
        .iter()
        .enumerate()
        .map(|(i, a)| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(cx.state),
                a.target(cx.state),
            )
        })
        .collect::<Vec<_>>();
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single("Fast window — play a card or pass", options)
            .skippable(),
        resume_token: ResumeToken(0),
    }
}

/// Collect every fast play currently eligible across all investigators: Fast
/// cards in hand ([`check_play_card`] `Ok` + `is_fast`) and 0-action
/// [`Trigger::Activated`] abilities on cards in play ([`check_activate_ability`]
/// `Ok`). MUST be called with the `FastWindow` on top of the stack so
/// `check_play_card`'s `permits_fast` gate applies to the right window (#476).
///
/// Returns the plays as [`TurnAction`]s in deterministic (investigator,
/// hand-index / ability-index) order — the same shape the open-turn menu
/// dispatches via `dispatch_turn_action`, so the #476 fast-window prompt reuses
/// that dispatch path verbatim. Empty when the registry isn't installed.
pub(super) fn enumerate_fast_plays(state: &GameState) -> Vec<TurnAction> {
    let mut out = Vec::new();
    let Some(reg) = crate::card_registry::current() else {
        return out;
    };
    for (&inv_id, inv) in &state.investigators {
        // Fast events / Fast assets in hand.
        for hand_idx_usize in 0..inv.hand.len() {
            let Ok(hand_index) = u8::try_from(hand_idx_usize) else {
                break;
            };
            if let Ok(result) = check_play_card(state, inv_id, hand_index) {
                if result.is_fast {
                    out.push(TurnAction::PlayCard {
                        investigator: inv_id,
                        hand_index,
                    });
                }
            }
        }
        // 0-action Activated abilities on every source this investigator can
        // reach. The rules bullets are written once for `[free]`, `[reaction]`
        // and `[action]` together, so the fast window consults the same
        // reachability predicate the turn menu does (#707).
        for (source, code) in crate::engine::ability_source::reachable_source_codes(state, inv_id) {
            let Some(abilities) = (reg.abilities_for)(&code) else {
                continue;
            };
            for (ab_idx, ability) in abilities.iter().enumerate() {
                let Trigger::Activated { action_cost: 0, .. } = ability.trigger else {
                    continue;
                };
                let Ok(ability_index) = u8::try_from(ab_idx) else {
                    break;
                };
                if check_activate_ability(state, inv_id, source, ability_index).is_ok() {
                    out.push(TurnAction::ActivateAbility {
                        investigator: inv_id,
                        source,
                        ability_index,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod check_play_card_tests {
    use super::*;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn check_play_card_returns_err_for_unknown_hand_index() {
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let err =
            check_play_card(&state, InvestigatorId(1), 0).expect_err("empty hand should reject");
        assert!(
            err.contains("hand_index"),
            "error should mention hand_index, got: {err}"
        );
    }

    #[test]
    fn check_play_card_returns_err_when_investigator_missing() {
        let state = GameStateBuilder::default().build();
        let err = check_play_card(&state, InvestigatorId(99), 0)
            .expect_err("missing investigator should reject");
        assert!(
            err.contains("not in state"),
            "error should say not in state, got: {err}"
        );
    }
}

#[cfg(test)]
mod trigger_matches_tests {
    use super::*;
    use crate::state::{CardInstanceId, EnemyId, LocationId};

    fn enemy_attacks(inv: InvestigatorId) -> TimingEvent {
        TimingEvent::EnemyAttacks {
            enemy: EnemyId(1),
            investigator: inv,
        }
    }

    /// The pattern↔condition pairings, which since #704 are **timing-free**:
    /// `trigger_matches` no longer takes a timing, so a card pairs with its
    /// condition identically in all three cells and the cell filtering happens
    /// in the scans. Deleting the `When` whitelist is what makes an
    /// `after`-an-enemy-attacks ability declarable at all.
    #[test]
    fn a_pattern_pairs_with_its_condition_in_every_cell() {
        let inv = InvestigatorId(1);
        let discover = TimingEvent::DiscoverClues {
            investigator: inv,
            location: LocationId(2),
            count: 1,
        };
        // EnemyAttacks ↔ EnemyAttacks — Dodge 01023 (`when`) and any
        // `at`/`after` ability on the same condition.
        assert!(trigger_matches(
            &enemy_attacks(inv),
            &EventPattern::EnemyAttacks,
            inv,
        ));
        // DiscoverClues ↔ DiscoverClues — Cover Up 01007.
        assert!(trigger_matches(
            &discover,
            &EventPattern::DiscoverClues,
            inv
        ));
        // A condition still only matches its own pattern.
        assert!(!trigger_matches(
            &enemy_attacks(inv),
            &EventPattern::DiscoverClues,
            inv,
        ));
    }

    #[test]
    fn round_ended_matches_its_own_pattern_board_scoped() {
        let lead = InvestigatorId(1);
        // RoundEnded ↔ RoundEnded — act 01109's group advance (#434).
        // Board-scoped: matches regardless of the candidate's controller.
        assert!(trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            lead,
        ));
        assert!(trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            InvestigatorId(2),
        ));
        // Another condition's pattern still does not match it.
        assert!(!trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::EnemyAttacks,
            lead,
        ));
    }

    /// An [`Assignment`](crate::state::Assignment) giving 1 damage to `inst` —
    /// the shape a `DamageAssigned` event carries when that card is a soaker.
    fn assignment_damaging(inst: CardInstanceId) -> crate::state::Assignment {
        let mut assignment = crate::state::Assignment::default();
        assignment.asset_damage.insert(inst, 1);
        assignment
    }

    /// Direct `trigger_matches` coverage for the `EnemyAttackDamagedSelf` soak
    /// pairing (Guard Dog 01021, C5b #237). The instance-level scoping (only an
    /// asset the assignment gives damage to fires) is enforced one layer up in
    /// `scan_pending_triggers` and exercised end-to-end in
    /// `crates/cards/tests/guard_dog_soak.rs` (which installs the real registry);
    /// what *this* layer owns since #727 is the card's narrowing to an enemy
    /// **attack**.
    #[test]
    fn soak_event_matches_only_the_self_soak_pattern() {
        let controller = InvestigatorId(1);
        let soak = TimingEvent::DamageAssigned {
            source: crate::state::DamageSource::EnemyAttack { enemy: EnemyId(1) },
            investigator: controller,
            assignment: assignment_damaging(CardInstanceId(7)),
        };
        // The soak-self pattern matches the soak event. (C5b #237.)
        assert!(trigger_matches(
            &soak,
            &EventPattern::EnemyAttackDamagedSelf,
            controller,
        ));
        // …but not the same condition from a non-attack source: Guard Dog
        // retaliates to an enemy *attack*, not to treachery harm.
        let effect_harm = TimingEvent::DamageAssigned {
            source: crate::state::DamageSource::Effect,
            investigator: controller,
            assignment: assignment_damaging(CardInstanceId(7)),
        };
        assert!(!trigger_matches(
            &effect_harm,
            &EventPattern::EnemyAttackDamagedSelf,
            controller,
        ));
        // No other pattern matches the soak event.
        assert!(!trigger_matches(
            &soak,
            &EventPattern::EnemyDefeated {
                by_controller: false,
                code: None,
            },
            controller,
        ));
        // The soak pattern must NOT match a different event (guards the
        // arm ordering — the `=> true` arm is scoped to the soak event).
        let defeat = TimingEvent::EnemyDefeated {
            enemy: EnemyId(1),
            by: Some(controller),
            code: CardCode("01000".into()),
        };
        assert!(!trigger_matches(
            &defeat,
            &EventPattern::EnemyAttackDamagedSelf,
            controller,
        ));
    }
}

#[cfg(test)]
mod check_activate_ability_tests {
    use super::*;
    use crate::state::CardInstanceId;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn check_activate_ability_returns_err_for_unreachable_source() {
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let err = check_activate_ability(
            &state,
            InvestigatorId(1),
            AbilitySource::InPlay(CardInstanceId(999)),
            0,
        )
        .expect_err("a source the investigator cannot reach should reject");
        assert!(
            err.contains("cannot reach"),
            "error should say the source is unreachable, got: {err}"
        );
    }

    #[test]
    fn check_activate_ability_returns_err_when_investigator_missing() {
        let state = GameStateBuilder::default().build();
        let err = check_activate_ability(
            &state,
            InvestigatorId(99),
            AbilitySource::InPlay(CardInstanceId(1)),
            0,
        )
        .expect_err("missing investigator should reject");
        assert!(
            err.contains("not in state"),
            "error should say not in state, got: {err}"
        );
    }
}

#[cfg(test)]
mod any_fast_play_eligible_tests {
    use super::*;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn returns_false_when_no_investigators() {
        let state = GameStateBuilder::default().build();
        assert!(!any_fast_play_eligible(&state));
    }

    #[test]
    fn returns_false_when_hands_and_in_play_empty() {
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        assert!(!any_fast_play_eligible(&state));
    }
}

#[cfg(test)]
mod resolution_option_anchor_tests {
    use super::*;

    #[test]
    fn resolution_options_anchor_by_candidate_source() {
        use crate::engine::OptionTarget;
        use crate::state::{CardCode, CardInstanceId, InvestigatorId, ResolutionCandidate};
        let cands = vec![
            ResolutionCandidate {
                code: CardCode::new("_inplay"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::InPlay(CardInstanceId(9)),
            },
            ResolutionCandidate {
                code: CardCode::new("01022"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Hand,
            },
            ResolutionCandidate {
                code: CardCode::new("_board"),
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
        ];
        let opts = build_resolution_options(&cands, None, None);
        assert_eq!(
            opts[0].target,
            OptionTarget::CardInstance(CardInstanceId(9))
        );
        assert_eq!(
            opts[1].target,
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new("01022"),
            }
        );
        assert_eq!(opts[2].target, OptionTarget::Global);
    }

    #[test]
    fn board_candidate_matching_current_act_anchors_to_act() {
        use crate::engine::OptionTarget;
        use crate::state::{CardCode, InvestigatorId, ResolutionCandidate};
        let act = CardCode::new("01109");
        let cands = vec![
            ResolutionCandidate {
                code: act.clone(), // the round-end act-advance reaction
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
            ResolutionCandidate {
                code: CardCode::new("_other_board"), // some other board-wide reaction
                controller: InvestigatorId(1),
                ability_index: 0,
                source: CandidateSource::Board,
            },
        ];
        let opts = build_resolution_options(&cands, Some(&act), None);
        assert_eq!(opts[0].target, OptionTarget::Act);
        assert_eq!(opts[1].target, OptionTarget::Global);
    }

    #[test]
    fn candidate_anchor_maps_each_source() {
        use crate::engine::OptionTarget;
        use crate::state::{
            CardCode, CardInstanceId, InvestigatorId, LocationId, ResolutionCandidate,
        };
        let act = CardCode::new("01109");
        let agenda = CardCode::new("01105");
        let inplay = ResolutionCandidate::new(
            CardCode::new("01020"),
            InvestigatorId(1),
            0,
            CandidateSource::InPlay(CardInstanceId(5)),
        );
        // A location's own forced ability (the Attic, 01113) anchors to its map
        // node, independent of the current act (#553).
        let location = ResolutionCandidate::new(
            CardCode::new("01113"),
            InvestigatorId(1),
            0,
            CandidateSource::Location(LocationId(7)),
        );
        let hand = ResolutionCandidate::new(
            CardCode::new("01022"),
            InvestigatorId(2),
            0,
            CandidateSource::Hand,
        );
        let board_act =
            ResolutionCandidate::new(act.clone(), InvestigatorId(1), 0, CandidateSource::Board);
        let board_other = ResolutionCandidate::new(
            CardCode::new("_other"),
            InvestigatorId(1),
            0,
            CandidateSource::Board,
        );
        // A board candidate whose code is the current agenda anchors to the agenda
        // card (What's Going On?! 01105's on-advance forced, #556).
        let board_agenda =
            ResolutionCandidate::new(agenda.clone(), InvestigatorId(1), 0, CandidateSource::Board);
        assert_eq!(
            candidate_anchor(&inplay, Some(&act), Some(&agenda)),
            OptionTarget::CardInstance(CardInstanceId(5))
        );
        assert_eq!(
            candidate_anchor(&hand, Some(&act), Some(&agenda)),
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(2),
                code: CardCode::new("01022"),
            }
        );
        // An act-coded board candidate still wins Act even with an agenda present.
        assert_eq!(
            candidate_anchor(&board_act, Some(&act), Some(&agenda)),
            OptionTarget::Act
        );
        assert_eq!(
            candidate_anchor(&board_other, Some(&act), Some(&agenda)),
            OptionTarget::Global
        );
        assert_eq!(
            candidate_anchor(&location, Some(&act), Some(&agenda)),
            OptionTarget::Location(LocationId(7))
        );
        assert_eq!(
            candidate_anchor(&board_agenda, Some(&act), Some(&agenda)),
            OptionTarget::Agenda
        );
    }
}

#[cfg(test)]
mod open_fast_window_tests {
    use super::*;
    use crate::state::{FastWindowKind, PhaseStep};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn open_fast_window_with_no_eligibility_auto_skips_inline() {
        // No reactions, no Fast-eligible cards → auto-skip: window
        // opens and closes without ever landing on state.open_windows.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            // The MythosAfterDraws window now closes onto the MythosPhase anchor
            // (slice 1a); stage it so the auto-skip continuation has its frame.
            .with_phase_anchor(crate::state::Continuation::MythosPhase {
                resume: crate::state::MythosResume::AfterDraws,
            })
            .build();
        let mut events = Vec::new();
        open_fast_window(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        );

        assert!(
            state.open_windows().is_empty(),
            "auto-skip must not leave the window on the stack"
        );
    }

    /// With no fast-playable card or 0-cost ability available, the enumeration is
    /// empty (the auto-skip path). The positive case — a real Fast card becoming
    /// a `PlayCard` candidate — is covered by the Task 5 integration regression,
    /// because game-core's test registry exposes no playable cards.
    #[test]
    fn enumerate_fast_plays_empty_when_nothing_eligible() {
        let inv = crate::state::InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_phase(crate::state::Phase::Investigation)
            .with_active_investigator(inv)
            .with_investigator(test_investigator(1))
            .build();
        assert!(enumerate_fast_plays(&state).is_empty());
    }
}

#[cfg(test)]
mod candidate_source_present_tests {
    use super::*;
    use crate::state::CardInstanceId;
    use crate::state::{CardInPlay, LocationId};
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};

    const INV: InvestigatorId = InvestigatorId(1);
    const SOME_CODE: &str = "_synth_card";

    fn candidate(source: CandidateSource) -> ResolutionCandidate {
        ResolutionCandidate::new(CardCode::new(SOME_CODE), INV, 0, source)
    }

    #[test]
    fn a_hand_candidate_is_present_only_while_the_code_is_in_hand() {
        let mut inv = test_investigator(1);
        inv.hand.push(CardCode::new(SOME_CODE));
        let state = GameStateBuilder::default().with_investigator(inv).build();
        assert!(candidate_source_present(
            &state,
            &candidate(CandidateSource::Hand)
        ));

        let mut drained = state.clone();
        drained.investigators.get_mut(&INV).unwrap().hand.clear();
        assert!(!candidate_source_present(
            &drained,
            &candidate(CandidateSource::Hand)
        ));
    }

    #[test]
    fn an_in_play_candidate_is_present_only_while_its_instance_is() {
        let instance = CardInstanceId(7);
        let mut inv = test_investigator(1);
        inv.cards_in_play
            .push(CardInPlay::enter_play(CardCode::new(SOME_CODE), instance));
        let state = GameStateBuilder::default().with_investigator(inv).build();
        assert!(candidate_source_present(
            &state,
            &candidate(CandidateSource::InPlay(instance))
        ));
        // A different instance of the same code is a different candidate.
        assert!(!candidate_source_present(
            &state,
            &candidate(CandidateSource::InPlay(CardInstanceId(8)))
        ));
    }

    #[test]
    fn a_board_candidate_is_present_only_while_it_is_the_current_act_or_agenda() {
        // No act/agenda deck seeded, so no board code can match.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .build();
        assert!(!candidate_source_present(
            &state,
            &candidate(CandidateSource::Board)
        ));
    }

    #[test]
    fn a_location_candidate_tracks_its_location() {
        let loc = LocationId(10);
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_location(test_location(10, "Study"))
            .build();
        assert!(candidate_source_present(
            &state,
            &candidate(CandidateSource::Location(loc))
        ));
        assert!(!candidate_source_present(
            &state,
            &candidate(CandidateSource::Location(LocationId(11)))
        ));
    }
}

#[cfg(test)]
mod withdraw_suppressed_candidates_tests {
    use super::*;
    use crate::state::CardInstanceId;
    use crate::state::LocationId;
    use crate::test_support::{test_investigator, GameStateBuilder};

    const INV: InvestigatorId = InvestigatorId(1);
    const CODE: &str = "_synth_reaction";

    fn discovery() -> TimingEvent {
        TimingEvent::DiscoverClues {
            investigator: INV,
            location: LocationId(10),
            count: 1,
        }
    }

    /// A one-candidate **reaction** window on `bucket`. The common case; see
    /// [`state_with_window_of`] for the other axes.
    fn state_with_window(bucket: EventTiming, prevented: bool) -> GameState {
        state_with_window_of(discovery(), bucket, TimingMode::Reaction, prevented)
    }

    /// A one-candidate window over `event`, in `bucket`, of `mode`, on top of a
    /// minimal state whose `pending_cancellation` is `prevented`.
    fn state_with_window_of(
        event: TimingEvent,
        bucket: EventTiming,
        mode: TimingMode,
        prevented: bool,
    ) -> GameState {
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_active_investigator(INV)
            .build();
        state.pending_cancellation = prevented;
        state.continuations.push(Continuation::TimingPointWindow {
            event,
            bucket,
            mode,
            candidates: vec![ResolutionCandidate::new(
                CardCode::new(CODE),
                INV,
                0,
                CandidateSource::InPlay(CardInstanceId(1)),
            )],
        });
        state
    }

    fn withdraw(state: &mut GameState) -> (usize, Vec<Event>) {
        let mut events = Vec::new();
        let n = withdraw_suppressed_candidates(&mut Cx {
            state,
            events: &mut events,
        });
        (n, events)
    }

    fn remaining(state: &GameState) -> usize {
        state
            .continuations
            .last()
            .and_then(Continuation::pending_candidates)
            .expect("the window is still on top")
            .len()
    }

    /// The Dodge 01023 shape: the condition was prevented in this very cell, so
    /// the sibling still pending in the window is withdrawn rather than offered.
    #[test]
    fn a_prevented_condition_empties_its_when_cell_window() {
        let mut state = state_with_window(EventTiming::When, true);
        let (n, events) = withdraw(&mut state);
        assert_eq!(n, 1, "the pending sibling must be withdrawn");
        assert_eq!(remaining(&state), 0);
        crate::assert_event!(
            events,
            Event::ReactionOptionLapsed {
                reason: LapseReason::ConditionPrevented,
                ..
            }
        );
    }

    /// No signal, no suppression — the ordinary window is left entirely alone
    /// (its own re-scan withdrawal is `withdraw_lapsed_candidates`' job).
    #[test]
    fn an_unprevented_condition_withdraws_nothing() {
        let mut state = state_with_window(EventTiming::When, false);
        let (n, events) = withdraw(&mut state);
        assert_eq!(n, 0);
        assert_eq!(remaining(&state), 1);
        assert!(events.is_empty(), "no lapse to report; events = {events:?}");
    }

    /// Scoped to the `when` cell: it is the only one whose abilities resolve
    /// before the condition does, so a signal seen at an `at` or `after` window
    /// belongs to something else and must not empty it.
    #[test]
    fn a_later_cells_window_is_untouched() {
        for bucket in [EventTiming::At, EventTiming::After] {
            let mut state = state_with_window(bucket, true);
            let (n, _) = withdraw(&mut state);
            assert_eq!(n, 0, "{bucket:?} must not be suppressed");
            assert_eq!(remaining(&state), 1);
        }
    }

    /// The forced arm: a 2+ lead-ordered run (#213) empties the same way, so it
    /// closes itself instead of demanding a pick for a condition that is no
    /// longer happening. The unit half of the shape Dodge 01023's ruling is
    /// stated about; the corpus half — dodging Silver Twilight Acolyte 01102 —
    /// is `crates/cards/tests/dodge.rs`.
    #[test]
    fn a_forced_run_in_the_when_cell_empties_too() {
        let mut state =
            state_with_window_of(discovery(), EventTiming::When, TimingMode::Forced, true);
        let (n, events) = withdraw(&mut state);
        assert_eq!(
            n, 1,
            "a forced run is withdrawn from like a reaction window"
        );
        assert_eq!(remaining(&state), 0);
        crate::assert_event!(
            events,
            Event::ReactionOptionLapsed {
                reason: LapseReason::ConditionPrevented,
                ..
            }
        );
    }

    /// A **caller-owned** condition is left alone: it has already mutated the
    /// board by the time any window opens, so a prevention signal in flight is
    /// not its to consume. The enemy attack used to be this fixture and #704
    /// migrated it; the soak condition stood in until #727 replaced it with the
    /// coordinator-owned `DamageAssigned`/`DamagePlaced` pair, so `EnteredPlay`
    /// (Research Librarian 01032's window) stands in now.
    #[test]
    fn a_caller_owned_conditions_window_is_untouched() {
        let entered = TimingEvent::EnteredPlay {
            instance: crate::state::CardInstanceId(1),
            controller: INV,
        };
        debug_assert!(matches!(
            entered.condition_resolution(),
            super::super::emit::ConditionResolution::Caller
        ));
        let mut state =
            state_with_window_of(entered, EventTiming::When, TimingMode::Reaction, true);
        let (n, events) = withdraw(&mut state);
        assert_eq!(n, 0, "a caller-owned condition is not suppressed here");
        assert_eq!(remaining(&state), 1);
        assert!(events.is_empty(), "no lapse to report; events = {events:?}");
    }

    /// A frame that is not a resolution window at all — here the `when` cell's
    /// own [`Continuation::TimingPoint`], which is what the stack holds while a
    /// single forced ability resolves inline. No candidates to withdraw, so the
    /// callers need no guard of their own.
    #[test]
    fn a_non_window_frame_is_a_no_op() {
        let mut state = GameStateBuilder::default().build();
        state.pending_cancellation = true;
        state.continuations.push(Continuation::TimingPoint {
            event: discovery(),
            bucket: EventTiming::When,
            sub: crate::state::TimingSub::Reaction,
        });
        let (n, events) = withdraw(&mut state);
        assert_eq!(n, 0);
        assert!(events.is_empty());
    }
}
