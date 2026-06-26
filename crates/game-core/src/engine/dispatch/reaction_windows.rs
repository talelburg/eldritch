//! Reaction-window and fast-window helpers.
//!
//! Contains the open/scan/fire/close pipeline for after-event reaction
//! windows ([`queue_reaction_window`], [`scan_pending_triggers`],
//! [`trigger_matches`], [`open_queued_reaction_window`],
//! [`resume_reaction_window`], [`fire_pending_trigger`],
//! [`bump_usage_counter`], [`close_reaction_window`],
//! [`run_reaction_continuation`]) and the fast-window eligibility checks
//! ([`check_play_card`], [`check_activate_ability`],
//! [`any_fast_play_eligible`], [`open_fast_window`]).

use std::borrow::Cow;

use crate::action::InputResponse;
use crate::card_data::{CardMetadata, CardType};
use crate::card_registry;
use crate::dsl::{Ability, EventPattern, EventTiming, Trigger, TriggerKind};
use crate::engine::enumerate::TurnAction;
use crate::engine::TimingEvent;
use crate::state::TimingMode;
use crate::state::{
    CandidateSource, CardCode, CardInstanceId, Continuation, FastActorScope, FastWindowKind,
    GameState, InvestigatorId, Phase, ResolutionCandidate, Status,
};

use super::super::evaluator::{push_effect, EvalContext};
use super::super::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use super::Cx;

/// Queue a reaction window of the given `kind` if any candidate matches —
/// an in-play card with a matching `Trigger::OnEvent` ability *or* (Axis C,
/// #335) a Fast event in hand whose play-instruction matches. No-op when the
/// registry isn't installed or nothing matches.
///
/// Pushes the window onto [`GameState::open_windows`], symmetric with the
/// Fast-window path ([`open_fast_window`]). If no candidate matches the
/// function returns early without pushing — the window never opens.
///
/// The hand events are appended *after* the in-play triggers in the single
/// `pending_triggers` list, so they are offered as options after the
/// triggers; each carries [`CandidateSource::Hand`] so the fire path *plays*
/// it rather than firing an in-play ability.
///
/// The window suspends the surrounding driver
/// (today, [`advance`]) at its next step boundary: after the
/// emit here the driver sees a non-empty `open_windows` stack and
/// returns [`EngineOutcome::AwaitingInput`] so the player can act.
///
/// Idempotency: if a window is already queued for this apply, the new
/// `kind` overwrites it. Phase-3 actions only emit one defeating
/// event per apply (a single Fight's `damage_enemy` call), so this case
/// doesn't arise; the overwrite is a loud-on-debug placeholder
/// rather than silent stacking — multi-window queueing lands when a
/// multi-defeat effect arrives.
pub(super) fn queue_reaction_window(cx: &mut Cx, event: &crate::engine::TimingEvent) {
    // Single-bucket events open their window at their natural timing; the
    // coordinator opens per-cell windows directly (#434, Task 3).
    let bucket = event.reaction_bucket();
    let mut candidates = scan_pending_triggers(cx.state, event, bucket);
    // Axis C (#335): the window also opens for a matching Fast event in hand,
    // so a defeat with Evidence! in hand (and no in-play reaction) still opens
    // the after-defeat window. Hand plays are offered after the in-play
    // triggers.
    candidates.extend(scan_hand_fast_events(cx.state, event, bucket));
    if candidates.is_empty() {
        return;
    }
    // Reaction windows admit any investigator's Fast actions (RR: Fast may be
    // played at any player window) — encoded by `mode: Reaction` (the former
    // `FastActorScope::Any` binding). Multi-window nesting is structural.
    cx.state
        .continuations
        .push(Continuation::TimingPointWindow {
            event: event.clone(),
            mode: crate::state::TimingMode::Reaction,
            candidates,
        });
}

/// All reaction candidates (in-play + hand Fast + current act/agenda) for
/// `event` at `bucket` — the `EmitEvent`/`TimingPoint` coordinator's per-cell
/// reaction scan (#434). Unlike [`queue_reaction_window`] (which reads the
/// event's *natural* bucket), the coordinator passes the cell it is resolving.
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
/// open it (the round-end `when` act-advance window, #434). Returns the
/// `AwaitingInput` from [`open_queued_reaction_window`]. Caller guarantees
/// `candidates` is non-empty (it checked, to decide open-vs-finish).
pub(super) fn open_reaction_run(
    cx: &mut Cx,
    event: &crate::engine::TimingEvent,
    candidates: Vec<ResolutionCandidate>,
) -> EngineOutcome {
    debug_assert!(
        !candidates.is_empty(),
        "open_reaction_run: caller must pass a non-empty candidate list"
    );
    cx.state
        .continuations
        .push(Continuation::TimingPointWindow {
            event: event.clone(),
            mode: crate::state::TimingMode::Reaction,
            candidates,
        });
    open_queued_reaction_window(cx)
}

/// Open the forced-resolution run (Axis-B T5b / #213): push a
/// `TimingPointWindow { mode: Forced }` holding the 2+ simultaneous forced
/// `candidates`, and present the lead investigator's order choice. The forced
/// run is mandatory (cannot be skipped) and admits no Fast plays. It carries no
/// resume continuation (#434): on close it returns `Done` and the `drive` loop
/// re-dispatches the exposed parent frame. The caller returns the `AwaitingInput`.
pub(super) fn open_forced_resolution(
    cx: &mut Cx,
    event: &crate::engine::TimingEvent,
    candidates: Vec<ResolutionCandidate>,
) -> EngineOutcome {
    cx.state
        .continuations
        .push(Continuation::TimingPointWindow {
            event: event.clone(),
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

/// Scan every investigator's `cards_in_play` **and the current act/agenda** for
/// `Trigger::OnEvent` reaction abilities matching `event` whose `EventTiming`
/// equals `bucket`, building a pending-trigger list in active-investigator-first
/// / turn-order resolution order (act/agenda board candidates, controlled by the
/// lead, appended last).
///
/// The `bucket` filter is what lets the round-end coordinator scan one timing
/// cell at a time (#434): `When` surfaces act 01109's group advance; `At`/`After`
/// surface nothing for `RoundEnded` (its doom is *forced*, not a reaction). For
/// single-bucket events the caller passes the event's [`reaction_bucket`] — the
/// abilities that pass `bucket` are exactly those that matched before, so it is
/// behaviour-preserving.
///
/// Returns an empty vec when the registry isn't installed (tests that
/// don't touch card data) or no cards match.
/// Whether a reaction `ability` may be offered, per its
/// [`Ability::eligibility`] tag (RR p.2: an ability can't initiate if its
/// effect won't change game state). No tag → eligible. A tag with no
/// resolvable predicate (registry absent / unknown tag) → suppressed, so a
/// half-installed host never offers a gated reaction it can't evaluate.
fn ability_eligible(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    let Some(tag) = ability.eligibility.as_deref() else {
        return true;
    };
    let Some(reg) = card_registry::current() else {
        return false;
    };
    let Some(pred) = (reg.native_eligibility_for)(tag) else {
        return false;
    };
    let ctx = EvalContext::for_controller_with_optional_source(controller, source.instance());
    pred(state, &ctx)
}

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
        // "When YOU would discover … at YOUR location" (Cover Up 01007, Axis D
        // #336): the reaction's controller is the discoverer and must be at the
        // discovery location. (The per-card `clues > 0` potential gate is in the
        // card loop below.)
        if let TimingEvent::WouldDiscoverClues {
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
            // Self-binding: for `EnemyAttackDamagedSelf` only the soaked asset
            // instance may trigger. All other instances are skipped here — the
            // pattern match in `trigger_matches` handles the pattern pairing;
            // this filter enforces the "self = the soaked asset" scoping (Guard
            // Dog 01021, C5b #237). Other events pass all instances through.
            if let TimingEvent::EnemyAttackDamagedSelf { asset, .. } = event {
                if card.instance_id != *asset {
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
                if !trigger_matches(event, pattern, *timing, id) {
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
                || !trigger_matches(event, pattern, *timing, lead)
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
                if !trigger_matches(event, pattern, *timing, id) {
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
/// `EventTiming::When` interrupt timing ("Forced — when X would Y") fires
/// only on the Before-windows matched below; the general after-event
/// reaction pipeline ignores it.
fn trigger_matches(
    event: &TimingEvent,
    pattern: &EventPattern,
    timing: EventTiming,
    controller: InvestigatorId,
) -> bool {
    // When-timing windows fire only for their exact event/pattern pairing (Axis
    // D #336); the "at your location" / eligibility scoping lives in the scans.
    match timing {
        EventTiming::When => {
            return matches!(
                (event, pattern),
                (TimingEvent::EnemyAttacks { .. }, EventPattern::EnemyAttacks)
                    | (
                        TimingEvent::WouldDiscoverClues { .. },
                        EventPattern::WouldDiscoverClues
                    )
                    // "When the round ends, investigators … may … advance" — act
                    // 01109's group advance (#434). A board-scoped reaction (no
                    // per-controller narrowing); the contributor scoping lives in
                    // the native + the round-end coordinator's `When` cell.
                    | (TimingEvent::RoundEnded, EventPattern::RoundEnded)
            );
        }
        // No `At`-timed reaction exists yet; treat it like `After` (fall through
        // to pattern matching). Dormant.
        EventTiming::At | EventTiming::After => {}
    }
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
        // The soaked-asset self-binding is enforced by the instance filter in
        // `scan_pending_triggers` (only the `asset` instance reaches here). Sole
        // consumer: Guard Dog 01021's retaliate reaction. (C5b #237.)
        (TimingEvent::EnemyAttackDamagedSelf { .. }, EventPattern::EnemyAttackDamagedSelf) => true,
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
        // Scoped to the entered card's owner; the self-instance scoping is in
        // the scan (Research Librarian 01032).
        (
            TimingEvent::EnteredPlay {
                controller: window_controller,
                ..
            },
            EventPattern::EnteredPlay,
        ) => *window_controller == controller,
        // Every other (event, pattern) pairing opens no reaction. The
        // forced-only events (PhaseEnded / ActAdvanced / AgendaAdvanced /
        // RoundEnded / EndOfTurn / GameEnd / EnteredLocation / LeftLocation /
        // EnteredLocation) never open a reaction window; the `When`-timing
        // events (EnemyAttacks / WouldDiscoverClues) returned above.
        _ => false,
    }
}

/// Build the structured option list for a resolution frame: one
/// [`ChoiceOption`] per pending candidate, in `pending_triggers` order.
/// `OptionId(i)` is the index into the returned list — the Axis-A convention
/// shared with [`super::choice`]. The label distinguishes a hand Fast-event
/// play ([`CandidateSource::Hand`]) from an in-play reaction.
fn build_resolution_options(candidates: &[ResolutionCandidate]) -> Vec<ChoiceOption> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, cand)| {
            let label = match cand.source {
                CandidateSource::Hand => format!("Play {} from hand", cand.code),
                CandidateSource::InPlay(_) | CandidateSource::Board => {
                    format!("Resolve reaction: {}", cand.code)
                }
            };
            ChoiceOption {
                id: OptionId(u32::try_from(i).expect("option count fits in u32")),
                label,
            }
        })
        .collect()
}

/// Return [`AwaitingInput`] for the already-open reaction window at
/// the top of [`GameState::open_windows`]. Called by [`advance`]
/// at a step boundary when an earlier step queued a window via
/// [`queue_reaction_window`].
///
/// The window is pushed onto the stack by [`queue_reaction_window`]
/// (not here), at queue time, symmetric with the [`open_fast_window`] path.
pub(crate) fn open_queued_reaction_window(cx: &mut Cx) -> EngineOutcome {
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
    let options = build_resolution_options(
        window
            .pending_candidates()
            .expect("open_queued_reaction_window: top window has candidates"),
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
    // For `AfterEnemyAttackDamagedAsset` windows, bind the attacking
    // enemy into the context so Guard Dog's native retaliate
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
        Some(crate::engine::TimingEvent::EnemyAttackDamagedSelf { enemy, .. }) => {
            eval_ctx.set_attacking_enemy(*enemy);
        }
        // For `WouldDiscoverClues`, bind the would-be discovery count so the
        // replacement effect (Cover Up's "discard that many") discards the
        // right number. Mirrors `attacking_enemy`. TODO(#368): `count` is the
        // requested, not the capped, count.
        Some(crate::engine::TimingEvent::WouldDiscoverClues { count, .. }) => {
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
/// Commences the play via the shared
/// [`super::cards::begin_event_play`] (emit [`crate::Event::CardPlayed`], leave hand,
/// stash in [`GameState::pending_played_event`] — RR Appendix I step 3), then
/// pushes a [`Continuation::PlayFromHand`] frame (above the live reaction window)
/// and the `OnEvent` effect for the drive loop. On the effect's completion,
/// [`super::cards::dispose_play_from_hand`] flushes the event to discard (RR
/// Appendix I step 4) and the window beneath resumes its candidate scan (Slice D
/// #423).
///
/// Charges no resource cost, matching [`super::cards::play_card`] (Slice 1
/// does not model play-cost resources). The caller has already removed the
/// candidate from the run, so a suspending effect's resume drives the
/// remaining siblings, not this play again.
fn play_fast_event(cx: &mut Cx, candidate: &ResolutionCandidate) -> EngineOutcome {
    let controller = candidate.controller;
    // Find the event in the controller's hand by code (first match — copies
    // are fungible; resolving by code avoids stale indices after a prior play).
    let hand_idx = cx
        .state
        .investigators
        .get(&controller)
        .and_then(|inv| inv.hand.iter().position(|c| *c == candidate.code))
        .unwrap_or_else(|| {
            unreachable!(
                "play_fast_event: candidate {candidate:?} vanished from \
                 {controller:?}'s hand between scan and play"
            )
        });
    super::cards::begin_event_play(cx, controller, hand_idx);

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

    // Push the event's disposal frame (above the window), then push its effect
    // for the drive loop. On the effect's completion, PlayFromHand disposal
    // flushes the event (RR Appendix I step 4) and the window beneath resumes
    // its candidate scan. `hand_idx` is moot for an event (begin_event_play
    // already removed + stashed it); pass it for the frame's shape. (Slice D #423.)
    cx.state
        .continuations
        .push(crate::state::Continuation::PlayFromHand {
            investigator: controller,
            code: candidate.code.clone(),
            hand_index: u8::try_from(hand_idx).unwrap_or(0),
        });
    push_effect(cx, &effect, eval_ctx);
    EngineOutcome::Done
}

/// Advance the resolution run on **top** of the stack after one of its
/// candidates resolved: close it (running its continuation) when none remain,
/// else re-emit the pick prompt. Called by the `drive` loop's window arm — the
/// window being driven is always the top frame (the stack-is-resolution-order
/// invariant), so there is no index to thread.
pub(super) fn advance_resolution(cx: &mut Cx) -> EngineOutcome {
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
    let options = build_resolution_options(candidates);
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
/// on `investigator_card.ability_usage`). `Board` and `Hand` candidates carry
/// no usage limits and are `unreachable!` here.
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
        CandidateSource::Board | CandidateSource::Hand => unreachable!(
            "bump_usage_counter: a usage-limited candidate must be an in-play instance \
             (board / hand candidates carry no usage limits); candidate {trigger:?}"
        ),
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

    // A window runs its kind-specific continuation
    // (e.g. MythosAfterDraws → mythos_phase_end). A framework window keys its
    // continuation off its `FastWindowKind`; a `TimingPointWindow` reaction
    // window keys off its `TimingEvent` (#433). The forced run (#213) carries no
    // continuation (#434): it returns `Done` and the `drive` loop re-dispatches
    // the exposed parent frame (the coordinator's `TimingPoint`, the
    // `InvestigatorTurn { ending }` frame, …). A reaction continuation may itself
    // suspend (e.g. an Enemy soak window), so propagate the outcome.
    let continuation = match &removed {
        Continuation::TimingPointWindow {
            event,
            mode: TimingMode::Reaction,
            ..
        } => {
            let event = event.clone();
            run_reaction_continuation(cx, &event)
        }
        Continuation::FastWindow { kind, .. } => run_fast_continuation(cx, *kind),
        // The forced run (`mode: Forced`): no continuation — the loop drives the
        // exposed frame next.
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

/// Continuation when a **reaction** window ([`Continuation::TimingPointWindow`])
/// closes, keyed on its [`TimingEvent`]. Called from [`close_reaction_window`]'s
/// pop path. Returns `Done`, or `AwaitingInput` when a body suspends (an Enemy
/// soak window, …).
fn run_reaction_continuation(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    match event {
        // Soak (C5b #237) + before-attack cancel (Axis D #336): re-enter the
        // enemy-attack loop the window suspended. `resume_enemy_attack` reads
        // its parked phase to either honour the cancel + deal the head attacker
        // (BeforeAttack) or drain the remaining attackers (AfterSoak).
        TimingEvent::EnemyAttackDamagedSelf { .. } | TimingEvent::EnemyAttacks { .. } => {
            super::combat::resume_enemy_attack(cx)
        }
        // Before-discover (Cover Up 01007, Axis D #336): perform the deferred
        // discovery unless a reaction cancelled it, then re-enter the in-flight
        // skill-test driver if any (Investigate's follow-up).
        TimingEvent::WouldDiscoverClues {
            investigator,
            location,
            count,
        } => resume_before_discover_window(cx, *investigator, *location, *count),
        // After-defeat / after-investigate / entered-play: no continuation work
        // here. Return `Done` so the `drive` loop dispatches the now-top frame —
        // a mid-resolution `SkillTest` resumes by being re-dispatched, *not* by an
        // inline `advance` call. (Contrast `run_fast_continuation`'s `SkillTest`
        // arm, which *does* call `advance` inline: that window's continuation *is*
        // the pre-advanced skill-test step, whereas a reaction window sits above a
        // separate `SkillTest` frame the loop owns. The asymmetry is intentional —
        // see `run_fast_continuation`.)
        // No continuation work here — the `drive` loop dispatches the now-top
        // frame. After-defeat / after-investigate / entered-play sit above a
        // separate `SkillTest` the loop owns; the round-end `when` act-advance
        // window (act 01109, #434) sits above the coordinator's `TimingPoint`,
        // which advances to its `Done` sub when re-dispatched.
        TimingEvent::EnemyDefeated { .. }
        | TimingEvent::SkillTestResolved { .. }
        | TimingEvent::EnteredPlay { .. }
        | TimingEvent::RoundEnded => EngineOutcome::Done,
        other => unreachable!("run_reaction_continuation: {other:?} opens no reaction window"),
    }
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

/// Resume after a before-discover-clues window closes (Cover Up
/// 01007, Axis D #336). If a reaction cancelled the discovery (Cover Up played
/// its `Seq[discard, Cancel]`), skip it; otherwise perform the deferred
/// discovery. Then, if a skill test is in flight (the dominant path:
/// Investigate's follow-up discovery), re-enter its driver — its continuation
/// was pre-advanced to `PostFollowUp` by `finish_skill_test` before the
/// follow-up suspended, so this picks up at teardown.
fn resume_before_discover_window(
    cx: &mut Cx,
    investigator: InvestigatorId,
    location: crate::state::LocationId,
    count: u8,
) -> EngineOutcome {
    let cancelled = std::mem::take(&mut cx.state.pending_cancellation);
    if !cancelled {
        crate::engine::evaluator::perform_discovery(cx, location, count, investigator);
    }
    // If a skill test is mid-flight (the dominant path: Investigate's follow-up
    // discovery), the `drive` loop dispatches it once this returns — no reach-down
    // into `skill_test::advance` (Slice C-plumbing).
    EngineOutcome::Done
}

/// Advance the enemy-phase cursor past `investigator` and open the next
/// window (C5b #237).
///
/// Extracted from the [`PhaseStep::BeforeInvestigatorAttacked`]
/// continuation so it runs from BOTH that arm (after the attack loop
/// completes without suspending) AND
/// [`super::combat::resume_enemy_attack`] (after a suspended loop
/// resumes and finishes). Advances the `EnemyPhase` anchor's `attacking`
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
    let (destination, abilities, is_fast, card_type) =
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
    Ok(super::PlayCheckResult {
        destination,
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

/// True if `effect` initiates a Fight at its top level.
///
/// Top-level only: no card yet fights in just one branch of a `Seq`/`If`
/// (.38 Special's `IntExpr` branches both fight, so the Fight node is
/// unconditionally top-level). Recurse here when such a card lands.
fn effect_initiates_fight(effect: &crate::dsl::Effect) -> bool {
    matches!(effect, crate::dsl::Effect::Fight { .. })
}

/// Reject an activation whose effect needs a target it cannot get, at the check
/// layer (before any cost is paid) so the rejection is honest for
/// `any_fast_play_eligible` and `Effect::Fight` / `DealDamageToEnemy` can treat
/// a missing target as an invariant violation.
///
/// - **Fight:** needs ≥1 enemy *at your location* (0 = no target, rejected
///   pre-cost; 2+ suspends to a `PickSingle` target-pick in the evaluator).
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
    effect: &crate::dsl::Effect,
) -> Result<(), Cow<'static, str>> {
    if effect_initiates_fight(effect)
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

/// Reject an ability mixing [`Cost::DiscardSelf`](crate::dsl::Cost::DiscardSelf)
/// with another source-referencing cost: `DiscardSelf` removes the source, so it
/// must be the sole such cost (Beat Cop / Knife list only it). `TODO(#301)` lift
/// if a card ever needs the combo.
fn reject_incompatible_costs(costs: &[crate::dsl::Cost]) -> Result<(), Cow<'static, str>> {
    use crate::dsl::Cost;
    if costs.iter().any(|c| matches!(c, Cost::DiscardSelf))
        && costs
            .iter()
            .any(|c| matches!(c, Cost::Exhaust | Cost::SpendUses { .. }))
    {
        return Err(
            "ActivateAbility: Cost::DiscardSelf cannot combine with Exhaust/SpendUses on the \
             same ability (it removes the source); TODO(#301) lift if a card needs the combo"
                .into(),
        );
    }
    Ok(())
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
    instance_id: CardInstanceId,
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
    let Some(in_play_pos) = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance_id)
    else {
        return Err(format!(
            "ActivateAbility: {investigator:?} has no in-play instance {instance_id:?}",
        )
        .into());
    };
    let source_code = inv.cards_in_play[in_play_pos].code.clone();
    let source_exhausted = inv.cards_in_play[in_play_pos].exhausted;
    let source_uses = inv.cards_in_play[in_play_pos].uses.clone();

    // Invariant: `resolve_activated_ability` currently returns only `Ok(...)`
    // (success) or `Err(EngineOutcome::Rejected { ... })` (validation failure).
    // If a future PR extends it to return `AwaitingInput` (e.g. for an ability
    // requiring target selection during validation), this `unreachable!()` will
    // panic; the validator's caller chain in `activate_ability` would need to be
    // redesigned to thread the `AwaitingInput` outcome back through
    // `check_activate_ability`'s `Result` shape. Mirrors the same invariant
    // comment on `resolve_play_target` in `check_play_card`.
    let (action_cost, costs, effect) =
        match super::abilities::resolve_activated_ability(&source_code, ability_index) {
            Ok(v) => v,
            Err(EngineOutcome::Rejected { reason }) => return Err(reason),
            Err(other) => {
                unreachable!("resolve_activated_ability returned non-Rejected outcome: {other:?}")
            }
        };

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
    check_effect_target_available(state, investigator, &effect)?;

    Ok(super::ActivateCheckResult {
        in_play_pos,
        source_code,
        action_cost,
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
        // 0-action Activated abilities on cards in play.
        for card in &inv.cards_in_play {
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (ab_idx, ability) in abilities.iter().enumerate() {
                let Trigger::Activated { action_cost: 0 } = ability.trigger else {
                    continue;
                };
                let Ok(ability_index) = u8::try_from(ab_idx) else {
                    break;
                };
                if check_activate_ability(state, inv_id, card.instance_id, ability_index).is_ok() {
                    out.push(TurnAction::ActivateAbility {
                        investigator: inv_id,
                        instance_id: card.instance_id,
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
    use crate::state::{EnemyId, LocationId};

    fn enemy_attacks(inv: InvestigatorId) -> TimingEvent {
        TimingEvent::EnemyAttacks {
            enemy: EnemyId(1),
            investigator: inv,
        }
    }

    #[test]
    fn before_pairs_match_only_their_own_when_event() {
        let inv = InvestigatorId(1);
        let would_discover = TimingEvent::WouldDiscoverClues {
            investigator: inv,
            location: LocationId(2),
            count: 1,
        };
        // EnemyAttacks ↔ EnemyAttacks (When) — Dodge 01023.
        assert!(trigger_matches(
            &enemy_attacks(inv),
            &EventPattern::EnemyAttacks,
            EventTiming::When,
            inv,
        ));
        // WouldDiscoverClues ↔ WouldDiscoverClues (When) — Cover Up 01007.
        assert!(trigger_matches(
            &would_discover,
            &EventPattern::WouldDiscoverClues,
            EventTiming::When,
            inv,
        ));
        // Wrong timing for the pairing → no match.
        assert!(!trigger_matches(
            &enemy_attacks(inv),
            &EventPattern::EnemyAttacks,
            EventTiming::After,
            inv,
        ));
        // A When event only matches its own pattern.
        assert!(!trigger_matches(
            &enemy_attacks(inv),
            &EventPattern::WouldDiscoverClues,
            EventTiming::When,
            inv,
        ));
    }

    #[test]
    fn round_ended_matches_only_its_when_reaction() {
        let lead = InvestigatorId(1);
        // RoundEnded ↔ RoundEnded (When) — act 01109's group advance (#434).
        // Board-scoped: matches regardless of the candidate's controller.
        assert!(trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            EventTiming::When,
            lead,
        ));
        assert!(trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            EventTiming::When,
            InvestigatorId(2),
        ));
        // The `at`/`after` buckets carry the round-end *forced* doom, not a
        // reaction — RoundEnded opens no reaction window there.
        assert!(!trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            EventTiming::At,
            lead,
        ));
        assert!(!trigger_matches(
            &TimingEvent::RoundEnded,
            &EventPattern::RoundEnded,
            EventTiming::After,
            lead,
        ));
    }

    /// Direct `trigger_matches` coverage for the `EnemyAttackDamagedSelf` soak
    /// pairing (Guard Dog 01021, C5b #237). The instance-level scoping (only the
    /// soaked `asset` instance fires) is enforced one layer up in
    /// `scan_pending_triggers` and exercised end-to-end in
    /// `crates/cards/tests/guard_dog_soak.rs` (which installs the real registry).
    #[test]
    fn soak_event_matches_only_the_self_soak_pattern() {
        let controller = InvestigatorId(1);
        let soak = TimingEvent::EnemyAttackDamagedSelf {
            asset: CardInstanceId(7),
            enemy: EnemyId(1),
            controller,
        };
        // The soak-self pattern matches the soak event. (C5b #237.)
        assert!(trigger_matches(
            &soak,
            &EventPattern::EnemyAttackDamagedSelf,
            EventTiming::After,
            controller,
        ));
        // No other pattern matches the soak event.
        assert!(!trigger_matches(
            &soak,
            &EventPattern::EnemyDefeated {
                by_controller: false,
                code: None,
            },
            EventTiming::After,
            controller,
        ));
        // Before timing never matches this After-only pairing.
        assert!(!trigger_matches(
            &soak,
            &EventPattern::EnemyAttackDamagedSelf,
            EventTiming::When,
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
            EventTiming::After,
            controller,
        ));
    }
}

#[cfg(test)]
mod check_activate_ability_tests {
    use super::*;
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn check_activate_ability_returns_err_for_missing_instance() {
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let err = check_activate_ability(&state, InvestigatorId(1), CardInstanceId(999), 0)
            .expect_err("missing instance should reject");
        assert!(
            err.contains("no in-play instance"),
            "error should say no in-play instance, got: {err}"
        );
    }

    #[test]
    fn check_activate_ability_returns_err_when_investigator_missing() {
        let state = GameStateBuilder::default().build();
        let err = check_activate_ability(&state, InvestigatorId(99), CardInstanceId(1), 0)
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
mod open_fast_window_tests {
    use super::*;
    use crate::state::{EnemyId, FastWindowKind, PhaseStep};
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

    #[test]
    fn run_reaction_continuation_for_no_continuation_kind_does_nothing() {
        // EnemyDefeated's reaction window has no continuation work. Closing it
        // must be a no-op (no events, no state change).
        let mut state = GameStateBuilder::default().build();
        let mut events = Vec::new();
        let result = run_reaction_continuation(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &TimingEvent::EnemyDefeated {
                enemy: EnemyId(1),
                by: None,
                code: crate::state::CardCode::new("01000"),
            },
        );
        assert_eq!(result, EngineOutcome::Done);
        assert!(
            events.is_empty(),
            "EnemyDefeated continuation must be a no-op; events = {events:?}"
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
