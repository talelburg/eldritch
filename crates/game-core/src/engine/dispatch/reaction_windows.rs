//! Reaction-window and fast-window helpers.
//!
//! Contains the open/scan/fire/close pipeline for after-event reaction
//! windows ([`scan_pending_triggers`],
//! [`trigger_matches`], [`open_queued_reaction_window`],
//! [`resume_reaction_window`], [`fire_pending_trigger`],
//! [`close_reaction_window`]) and the fast-window
//! eligibility checks
//! ([`check_play_card`], [`check_activate_ability`],
//! [`any_fast_play_eligible`], [`open_fast_window`]).

use std::borrow::Cow;

use card_dsl::card_data::{CardMetadata, CardType};
use card_dsl::dsl::{
    ActionDesignator, Cost, Effect, EnemyTarget, EventPattern, EventTiming, Trigger, TriggerKind,
    UsageLimit,
};

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::dispatch::abilities::ActivatedAbility;
use crate::engine::dispatch::emit::{ConditionResolution, TimingEvent};
use crate::engine::dispatch::initiation::{self, InitiationKind, Refusal};
use crate::engine::dispatch::{
    abilities, actions, cards, combat, cursor, phases, skill_test, slots, ActivateCheckResult,
    PlayCheckResult,
};
use crate::engine::enumerate::TurnAction;
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{abilities_in_effect, ability_source, designator, Cx};
use crate::event::{Event, LapseReason};
use crate::state::{
    AbilityAddress, AbilitySource, CandidateSource, CardCode, CardInstanceId, Continuation,
    DamageSource, FastActorScope, FastWindowFrame, FastWindowKind, GameState, InvestigatorId,
    Phase, PlayFromHandFrame, ResolutionCandidate, Status, TimingMode, TimingPointWindowFrame,
};

/// Push a reaction window frame for `candidates` at `bucket`. The shared push
/// behind [`open_reaction_run`] (which queues and then opens) and the coordinator's
/// per-cell scan.
///
/// Reaction windows admit any investigator's Fast actions (RR: Fast may be
/// played at any player window) — encoded by `mode: Reaction` (the former
/// `FastActorScope::Any` binding). Multi-window nesting is structural.
fn push_reaction_window(
    cx: &mut Cx,
    event: &TimingEvent,
    bucket: EventTiming,
    candidates: Vec<ResolutionCandidate>,
) {
    cx.state.continuations.push(TimingPointWindowFrame {
        event: event.clone(),
        bucket,
        mode: TimingMode::Reaction,
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
    event: &TimingEvent,
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
    event: &TimingEvent,
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
    event: &TimingEvent,
    bucket: EventTiming,
    candidates: Vec<ResolutionCandidate>,
) -> EngineOutcome {
    cx.state.continuations.push(TimingPointWindowFrame {
        event: event.clone(),
        bucket,
        mode: TimingMode::Forced,
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
    if card_registry::current().is_none() {
        return Vec::new();
    }
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
            // The funnel (#774/#772): the side in effect, plus whatever the
            // board grants this instance.
            let Some(abilities) = abilities_in_effect::for_source(
                state,
                AbilitySource::InPlay(card.instance_id),
                &card.code,
            ) else {
                continue;
            };
            for (address, ability) in &abilities {
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
                // Reaction candidates always have a source instance — an
                // in-play / threat-area card, or the investigator card itself
                // (#448 cp3a, now folded into `controlled_card_instances()`);
                // abilities resolve by `code`.
                let candidate = ResolutionCandidate {
                    code: card.code.clone(),
                    controller: id,
                    address: address.clone(),
                    source: CandidateSource::Ability(AbilitySource::InPlay(card.instance_id)),
                };
                // The initiation gate (ADR 0017): change-state, eligibility, the
                // "Limit X per [period]" counter, and cost.
                if initiation::check(state, &candidate, InitiationKind::Reaction).is_ok() {
                    pending.push(candidate);
                }
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
/// controller = the lead proxy, the first Active investigator in `turn_order`
/// ([`cursor::first_active_investigator`]; GLOSSARY "Lead investigator"), so
/// the reaction outlives the first seat's elimination and the gate's status
/// check never refuses it for that; the act's or the agenda's own
/// [`AbilitySource`] kind; no per-instance usage cap (acts have none). Empty
/// when the registry isn't installed, no investigator is Active, or nothing
/// matches.
fn scan_act_agenda_reactions(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
) -> Vec<ResolutionCandidate> {
    if card_registry::current().is_none() {
        return Vec::new();
    }
    let Some(lead) = cursor::first_active_investigator(state) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    // Each slot carries the source kind it *is* — the candidate never has to be
    // matched back to a board card by its code afterwards (#735).
    for (code, source) in [
        state
            .act_deck
            .get(state.act_index)
            .map(|a| (&a.code, AbilitySource::Act)),
        state
            .agenda_deck
            .get(state.agenda_index)
            .map(|a| (&a.code, AbilitySource::Agenda)),
    ]
    .into_iter()
    .flatten()
    {
        let Some(abilities) = abilities_in_effect::for_source(state, source, code) else {
            continue;
        };
        for (address, ability) in &abilities {
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
            let candidate = ResolutionCandidate {
                code: code.clone(),
                controller: lead,
                address: address.clone(),
                source: CandidateSource::Ability(source),
            };
            // The initiation gate (ADR 0017): suppress an act/agenda reaction
            // that cannot initiate (e.g. The Barrier 01109's round-end advance
            // when the Hallway group can't afford the clue threshold).
            if initiation::check(state, &candidate, InitiationKind::Reaction).is_ok() {
                hits.push(candidate);
            }
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
                let ability_index = u8::try_from(idx)
                    .expect("abilities vec exceeds u8::MAX — card-impl bug, abilities are tiny");
                let candidate = ResolutionCandidate {
                    code: code.clone(),
                    controller: id,
                    // A card in hand is not in play, so nothing grants to it.
                    address: AbilityAddress::Printed(ability_index),
                    source: CandidateSource::Hand,
                };
                // The initiation gate, as a *play* (ADR 0017): a Fast event is
                // played, so it is checked like any other play — its effect must
                // be able to change the game state (Evidence! 01022 at a 0-clue
                // location, #495), no "cannot play" may forbid it (Dissonant
                // Voices 01165, #917), and its resource cost must be payable
                // (#501). Filtering here keeps the offer honest; it is not the
                // binding check — the wallet is shared, so a sibling option can
                // empty it after this ran, and initiation re-asks (#568).
                if initiation::check(state, &candidate, InitiationKind::Play).is_err() {
                    continue;
                }
                plays.push(candidate);
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
            matches!(source, DamageSource::EnemyAttack { .. })
        }
        // "after you succeed/fail a skill test" — narrowed by outcome,
        // (optionally) test kind, and whether the card says *"**you**"*. Dr.
        // Milan 01033 is `{ Success, Some(Investigate), by_controller: true }`.
        //
        // `by_controller` is checked **here** rather than in an eligibility
        // predicate because this matcher runs before a candidate is minted: a
        // hard `*investigator == controller` would have made a reaction on
        // somebody else's test unreachable, whatever the card said afterwards.
        // With it `false` the pattern is unqualified and the card owns the
        // narrowing — Lita Chantler 01117's *"an investigator at your
        // location"* is her own native eligibility tag, since it is about the
        // board rather than about the event.
        (
            TimingEvent::SkillTestResolved {
                investigator,
                kind,
                outcome,
            },
            EventPattern::SkillTestResolved {
                outcome: p_out,
                kind: p_kind,
                by_controller,
            },
        ) => {
            (!*by_controller || *investigator == controller)
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
        // forced-only conditions (PhaseStarted / PhaseEnded / ActAdvanced / AgendaAdvanced /
        // EndOfTurn / GameEnd / EliminationGameEnd / EnteredLocation /
        // LeftLocation) never open a reaction window.
        _ => false,
    }
}

/// The board anchor for a resolution candidate's source: a Fast hand event by
/// code — every copy (#539); everything else through the one
/// [`AbilitySource`] → [`OptionTarget`] map, which the turn menu
/// (`TurnAction::target`) shares (#735).
///
/// Before the split this arm had to work out *which* board card a
/// `CandidateSource::Board` candidate was by comparing its code against the
/// current act and agenda, and fell through to an un-anchored option when
/// neither matched — which is what an attacking enemy's own forced ability hit
/// (Silver Twilight Acolyte 01102). The source now says which it is, so there is
/// nothing to derive and no fall-through: every candidate is anchored.
///
/// Shared by [`build_resolution_options`] and the forced-ack path.
pub(super) fn candidate_anchor(cand: &ResolutionCandidate) -> OptionTarget {
    match cand.source {
        CandidateSource::Hand => OptionTarget::HandCardByCode {
            investigator: cand.controller,
            code: cand.code.clone(),
        },
        CandidateSource::Ability(source) => source.into(),
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
            let id = OptionId(u32::try_from(i).expect("option count fits in u32"));
            // Label distinguishes a hand Fast-event play from an ability fired
            // in place; the anchor is the shared `candidate_anchor` (#553).
            let label = match cand.source {
                CandidateSource::Hand => format!("Play {} from hand", cand.code),
                CandidateSource::Ability(_) => format!("Resolve reaction: {}", cand.code),
            };
            ChoiceOption::new(id, label).at(candidate_anchor(cand))
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
        .top()
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
    cx.state
        .continuations
        .top_mut::<TimingPointWindowFrame>()
        .candidates = kept;
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
    let suppressed = match cx.state.continuations.top() {
        Some(Continuation::TimingPointWindow(TimingPointWindowFrame {
            event,
            bucket: EventTiming::When,
            candidates,
            ..
        })) if matches!(
            event.condition_resolution(),
            ConditionResolution::Coordinator(_)
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
        .top_mut::<TimingPointWindowFrame>()
        .candidates
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
    match state.continuations.top() {
        Some(Continuation::TimingPointWindow(TimingPointWindowFrame {
            event,
            bucket,
            mode: TimingMode::Reaction,
            ..
        })) => Some((event, *bucket)),
        _ => None,
    }
}

/// Why a withdrawn candidate lapsed, for the client log ([`LapseReason`],
/// #568). The withdrawal has already been decided by the re-scan in
/// [`withdraw_lapsed_candidates`]; this names the reason.
///
/// A source that is gone is [`LapseReason::SourceGone`]. Otherwise the
/// initiation gate is asked the question the scan asked of this candidate —
/// [`InitiationKind::Play`] for a Fast event in hand, which is played, and
/// [`InitiationKind::Reaction`] for an ability source — and its [`Refusal`] is
/// the reason. A candidate the gate still passes dropped out of the scan's own
/// scoping instead, which the gate does not own: [`LapseReason::OutOfScope`].
fn lapse_reason(state: &GameState, candidate: &ResolutionCandidate) -> LapseReason {
    if !candidate_source_present(state, candidate) {
        return LapseReason::SourceGone;
    }
    let kind = match candidate.source {
        CandidateSource::Hand => InitiationKind::Play,
        CandidateSource::Ability(_) => InitiationKind::Reaction,
    };
    match initiation::check(state, candidate, kind) {
        Ok(()) => LapseReason::OutOfScope,
        Err(refusal) => lapse_reason_for(&refusal),
    }
}

/// The [`LapseReason`] a gate [`Refusal`] reports as. A side no longer in
/// effect is [`LapseReason::SourceGone`]: the card is still there, but the
/// ability the option named is not.
fn lapse_reason_for(refusal: &Refusal) -> LapseReason {
    match refusal {
        Refusal::NotActive { .. } => LapseReason::NotActive,
        Refusal::SideNotInEffect => LapseReason::SourceGone,
        Refusal::NotEligible => LapseReason::NoLongerEligible,
        Refusal::NoStateChange => LapseReason::NoStateChange,
        Refusal::UsageLimitReached => LapseReason::UsageLimitReached,
        Refusal::PlayBanned { .. } => LapseReason::PlayBanned,
        Refusal::CostUnpayable(_) => LapseReason::CostUnpayable,
    }
}

/// Whether a withdrawn candidate's card is still where the scan found it — the
/// [`LapseReason::SourceGone`] probe.
///
/// A hand candidate is present while its code is still in the controller's
/// hand. Every other candidate names an [`AbilitySource`], and is present while
/// that source still names **the same card** the scan minted the candidate from
/// — `ability_source::source_card` walks the board, and the code comparison is
/// what makes an *advanced* act's queued ability lapse: `AbilitySource::Act`
/// names *the current* act, so the source has not vanished, it has become a
/// different card.
fn candidate_source_present(state: &GameState, candidate: &ResolutionCandidate) -> bool {
    match candidate.source {
        CandidateSource::Hand => state
            .investigators
            .get(&candidate.controller)
            .is_some_and(|inv| inv.hand.contains(&candidate.code)),
        CandidateSource::Ability(source) => ability_source::source_card(state, source)
            .is_some_and(|card| *card.code() == candidate.code),
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
        .top()
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
        .top()
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
                .top()
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
            .top()
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
            .top_frame_mut()
            .and_then(Continuation::pending_candidates_mut)
            .expect("fire_pending_trigger: top frame is an open window/run")
            .remove(pending_idx);
        return play_fast_event(cx, &trigger);
    }

    // Look up the ability fresh from the registry. The card may have
    // changed state between scan and fire (exhausted, used, …) but
    // its ability list is static, so registry lookup is sufficient.
    //
    // "Static" now means *static per side* (#774): a location shows its back's
    // abilities while unrevealed and its front's while revealed, so a reveal
    // between scan and fire would swap the vector the `ability_index` indexes.
    // That is a **game-state** change, not registry corruption, so it rejects
    // rather than panicking — `unreachable!()` is for invariant violations, and
    // "no location was revealed mid-window" is a property of today's corpus
    // (no back side declares a *triggered* ability; the Parlor 01115's is a
    // `Trigger::Constant`, inspected rather than scanned), not an invariant the
    // engine enforces. `candidate_still_offerable` above has already rejected a
    // source that left play, so what survives to here is the side flip.
    //
    // Abilities resolve by code (works for in-play instances and scenario
    // board cards alike); `source` is the firing instance, when any.
    let code = trigger.code.clone();
    let Some(ability) =
        abilities_in_effect::resolve(cx.state, trigger.source, &code, &trigger.address)
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: reaction-window PickSingle(OptionId({i})) names {code}, which no \
                 longer has the ability at {address:?} — the card was scanned on one side and \
                 fired on another, or a grant lapsed between the two. Re-read the current option \
                 list.",
                address = trigger.address,
            )
            .into(),
        };
    };

    // Thread the source instance (if any) into the EvalContext so effects
    // that self-reference (`DiscardSelf`) or push source-attributed state
    // resolve against the firing card. Board-card candidates (act / agenda)
    // have no source; hand candidates were handled above.
    let mut eval_ctx = EvalContext::for_controller_with_optional_source(
        trigger.controller,
        trigger.source.ability(),
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
        .top()
        .and_then(Continuation::window_timing_event)
    {
        Some(TimingEvent::DamageAssigned {
            source: DamageSource::EnemyAttack { enemy },
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
        Some(TimingEvent::DiscoverClues { count, .. }) => {
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
        .top_frame_mut()
        .and_then(Continuation::pending_candidates_mut)
        .expect("fire_pending_trigger: top frame is an open window/run")
        .remove(pending_idx);

    // Appendix I step 3: the ability attempts to initiate, so its use counts
    // now, before its effect is pushed — a use whose effects are cancelled still
    // counts. The window frame beneath stays on top with its remaining
    // candidates and `advance_resolution` re-dispatches it once the effect (and
    // any nested skill test) pops. In-scope suspending forced effects (Frozen in
    // Fear 01164) carry no usage limit, so recording is a no-op for them.
    initiation::record_initiation(cx.state, &trigger, usage_limit);
    evaluator::push_effect(cx, &ability.effect, eval_ctx);
    EngineOutcome::Done
}

/// Play the hand Fast-event `candidate` from the open resolution run (Axis C,
/// #335) — the [`CandidateSource::Hand`] resolution of [`fire_pending_trigger`].
/// Commences the play via the shared [`super::cards::commence_play`] (emit
/// [`crate::event::Event::CardPlayed`], leave hand — RR Appendix I step 3), then pushes a
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
    cards::pay_play_cost(cx, controller, &candidate.code);
    let card = cards::commence_play(cx, controller, hand_idx);

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
    let index = candidate.address.printed_index().unwrap_or_else(|| {
        unreachable!(
            "play_fast_event: {:?} is a granted address on a hand candidate {:?}; nothing \
             grants to a card that is not in play",
            candidate.address, candidate.code,
        )
    });
    let effect = abilities
        .get(usize::from(index))
        .unwrap_or_else(|| {
            unreachable!(
                "play_fast_event: printed index {index} out of range for {:?}",
                candidate.code,
            )
        })
        .effect
        .clone();
    let eval_ctx = EvalContext::for_controller(controller);

    // Push the event's disposal frame (above the window) holding the card, then
    // push its effect for the drive loop. On the effect's completion,
    // PlayFromHand disposal places the event in discard (RR Appendix I step 4)
    // and the window beneath resumes its candidate scan. (Slice D #423.)
    cx.state.continuations.push(PlayFromHandFrame {
        investigator: controller,
        card: Some(card),
    });
    evaluator::push_effect(cx, &effect, eval_ctx);
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
        .top()
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
        Continuation::FastWindow(FastWindowFrame { kind, .. }) => run_fast_continuation(cx, *kind),
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
        FastWindowKind::Phase(_) => phases::anchor_on_child_pop(cx),
        FastWindowKind::SkillTest { .. } => skill_test::advance(cx),
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
    let next = cursor::next_active_investigator_after(cx.state, investigator);
    phases::open_attack_window(cx, next)
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
    cx.state.continuations.push(FastWindowFrame {
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

/// Gates RR p.19 slot capacity: Assets only; the only hard slot reject — a merely-full
/// slot is not rejected here, make-room at enter-play handles it.
fn check_play_slot_satisfiable(
    card_type: CardType,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    if card_type != CardType::Asset {
        return Ok(());
    }
    if let Some(slot) = slots::unsatisfiable_slot(code) {
        return Err(format!(
            "PlayCard: {code} needs more {slot:?} slots than the investigator has \
             (slot capacity exceeded; RR p.19)."
        )
        .into());
    }
    Ok(())
}

/// Pure-validation peer to [`play_card`]. Returns `Ok` if the named
/// card is currently playable by `investigator`, `Err(reason)` if
/// not.
///
/// *Whether* the card may be played is the initiation gate's answer, asked as
/// a play ([`initiation::check_play`]); this validator owns only the *when* and
/// the rest of what a play from the turn menu or a player window needs — the
/// hand index, reaction events, slots, the turn/Fast timing matrix and the
/// action point (ADR 0017).
///
/// Used by [`play_card`] (which then runs the mutation block on the
/// `Ok` payload) and by `any_fast_play_eligible` (which only
/// inspects `Ok` vs `Err`).
pub(crate) fn check_play_card(
    state: &GameState,
    investigator: InvestigatorId,
    hand_index: u8,
) -> Result<PlayCheckResult, Cow<'static, str>> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(format!("PlayCard: investigator {investigator:?} is not in state").into());
    };
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
    let (_destination, abilities, is_fast, card_type) = match cards::resolve_play_target(&code) {
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
                    kind: TriggerKind::Reaction,
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
    // The initiation gate, as a play (ADR 0017): the investigator is Active, an
    // event's effect can change the game state (#495), no "cannot play" forbids
    // the card's type (Dissonant Voices 01165, #852), and its resource cost can
    // be paid — Fast only skips the *action* cost (#501). Asked here rather than
    // in the `play_card` handler so every consumer of this validator — the
    // open-turn menu, `enumerate_fast_plays` and the handler — agrees, and a card
    // that can't be played is never *offered*.
    initiation::check_play(state, investigator, &code)?;
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
    //
    // The companion clause bounds the *other* end of the turn, and the gate
    // above is deliberately inclusive of it: the end of your turn has not left
    // your turn. `data/official-faq/Frequently_Asked_Questions.md`, on Agatha
    // Crane's end-of-turn reaction — *"You resolve Agatha Crane's ability at
    // the end of your turn, which is still during your turn. So you could play
    // an event such as Cryptic Research ([core] 43) with the text 'Fast. Play
    // only during your turn.'"* The engine already agrees by construction:
    // `resume_end_turn` clears `active_investigator` only *after* the
    // `EndOfTurn` forced/reaction run has resolved (`phases.rs`), so a Fast
    // play made from inside that run still sees itself as the active
    // investigator and passes this gate.
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
    Ok(PlayCheckResult {
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

/// Classify a printed play cost into a payable number of resources, or the
/// reason it has none. The three shapes and why they differ are spelled out on
/// [`initiation::play_cost_payable`]; this is the arm split on its own so it
/// can be tested without a registry.
pub(super) fn payable_play_cost(
    play_cost: Option<i8>,
    code: &CardCode,
) -> Result<u8, Cow<'static, str>> {
    match play_cost {
        // A negative cost is ArkhamDB's X sentinel, never a real price;
        // `u8::try_from` would silently make it free, so it rejects here.
        Some(cost) => u8::try_from(cost).map_err(|_| {
            Cow::from(format!(
                "PlayCard: {code} has an X cost, which is not yet modeled \
                 (TODO(#577): X needs a player-chosen amount)."
            ))
        }),
        None => Err(format!(
            "PlayCard: {code} has a printed cost of \"–\", so it has no cost \
             that can be paid and cannot be played."
        )
        .into()),
    }
}

/// Reject an activation that cannot get what it needs, at the check layer —
/// **before any cost is paid**, so the rejection is honest for
/// `any_fast_play_eligible` and the evaluator can treat a missing target as an
/// invariant violation.
///
/// Two halves, asked in the order the ability declares them:
///
/// - **The designated action** — delegated whole to
///   [`can_perform`](crate::engine::designator::can_perform), the single
///   predicate the basic-action handlers and the turn-menu enumerator also
///   read (#805). A **Fight** needs ≥1 enemy at your location (0 = no target,
///   rejected here; 2+ suspends to a `PickSingle` in the evaluator); an
///   **Investigate** needs a revealed location, so Flashlight 01087 cannot
///   spend its supply with nothing to investigate.
/// - **`DealDamageToEnemy`:** needs ≥1 enemy in the chosen scope (e.g. "at your
///   location"). ≥1 proceeds — 2+ suspends via the `Choose` resolver — so only
///   the empty case rejects here; this is why the effect is typed, not `Native`
///   (Beat Cop can't pay its discard-self cost for no legal target). `amount` is
///   not consulted (a degenerate `amount: 0` ability — none in scope — would
///   still require a target here even though its handler is a no-op).
fn check_activation_target_available(
    state: &GameState,
    investigator: InvestigatorId,
    designator: Option<&ActionDesignator>,
    effect: &Effect,
) -> Result<(), Cow<'static, str>> {
    if let Some(designator) = designator {
        designator::can_perform(state, investigator, designator)
            .map_err(|why| Cow::from(format!("ActivateAbility: {why}")))?;
    }
    if let Effect::DealDamageToEnemy {
        target: EnemyTarget::Chosen(choose),
        ..
    } = effect
    {
        if combat::enemies_in_scope(state, investigator, choose.scope).is_empty() {
            return Err(
                "ActivateAbility: a 'deal damage to an enemy at your location' ability \
                 needs at least one enemy at your location"
                    .into(),
            );
        }
    }
    Ok(())
}

/// The `ExtraActionCost` surcharge an activation owes, plus the
/// `first_each_round` sources to mark spent once it commits.
///
/// A bold action designator makes this an action of that type, so a surcharge
/// on that class applies here exactly as it does to the basic action (#754).
/// Official FAQ: *"Abilities with a bold action designator (like Fight, Evade
/// or Investigate) count as an action of that type."* Frozen in Fear 01164's
/// ruling names the case directly: *"Also applies to \[action\] card abilities
/// with action designators (**Move**, **Fight**, **Evade**)."* Read through the
/// same `action_surcharge` the basic-action handlers use, so the two can't
/// drift apart.
///
/// Scoped to action-cost abilities: the same ruling taxes *fast* designated
/// abilities too, which no corpus card can reach and which needs a decision
/// #754 didn't make (#759, and the `# Module gap` on Frozen in Fear's own
/// module).
fn designated_action_surcharge(
    state: &GameState,
    investigator: InvestigatorId,
    action_cost: u8,
    designator: Option<&ActionDesignator>,
) -> (u8, Vec<CardInstanceId>) {
    if action_cost == 0 {
        return (0, Vec::new());
    }
    match designator.and_then(ActionDesignator::action_class) {
        Some(class) => actions::action_surcharge(state, investigator, class),
        None => (0, Vec::new()),
    }
}

/// Render the initiation gate's [`Refusal`] in the activation validator's
/// voice, so its rejection reasons read as they did before the gate (#958).
///
/// The cost check already speaks it (`abilities::check_cost_payable`), and the
/// change-state refusal keeps its #639 wording, which cites the rule it
/// enforces — `glossary/Ability.md`, "Triggered Abilities": *"A triggered
/// ability can only be initiated if its effect has the potential to change the
/// game state, and its cost (if any) has the potential to be paid in full,
/// taking active cost modifiers into account."* Every other refusal takes the
/// validator's `ActivateAbility:` prefix.
fn activation_refusal(refusal: Refusal, code: &CardCode) -> Cow<'static, str> {
    match refusal {
        Refusal::CostUnpayable(reason) => reason,
        Refusal::NoStateChange => format!(
            "ActivateAbility: {code}'s effect cannot change the game state right now, so the \
             ability cannot be initiated (RR \"Ability\"/\"Costs\")."
        )
        .into(),
        other => format!("ActivateAbility: {}", Cow::from(other)).into(),
    }
}

/// Reject an ability mixing [`Cost::DiscardSelf`](card_dsl::dsl::Cost::DiscardSelf)
/// with another source-referencing cost: `DiscardSelf` removes the source, so it
/// must be the sole such cost (Beat Cop / Knife list only it). Deliberately
/// unlifted until a card needs the combo — no tracking issue on purpose (YAGNI);
/// whoever hits this rejection files one.
fn reject_incompatible_costs(costs: &[Cost]) -> Result<(), Cow<'static, str>> {
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
/// location has no instance (`initiation::record_initiation`'s `unreachable!`
/// says so for the reaction path). Making these sources activatable is what first puts that
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
    address: &AbilityAddress,
    usage_limit: Option<UsageLimit>,
) -> Result<(), Cow<'static, str>> {
    if usage_limit.is_none() {
        return Ok(());
    }
    // The second untrackable shape (#772): a **granted** ability. The counter is
    // keyed by printed index — `BTreeMap<u8, _>`, because a JSON object key is a
    // string and an enum key does not survive the wire — so a granted ability
    // has nowhere to record a use. Refused for the same reason as the first
    // shape: silently uncapping a printed limit is worse than saying so. No
    // corpus grant prints one; the Parlor 01115's Parley is unlimited.
    if address.printed_index().is_none() {
        return Err(format!(
            "ActivateAbility: {code}'s granted ability at {address:?} carries a usage limit, but \
             the per-instance usage counter is keyed by printed index; TODO(#829): a granted \
             ability with a printed limit needs its own counter key"
        )
        .into());
    }
    if source.instance().is_some() {
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
    costs: &[Cost],
) -> Result<(), Cow<'static, str>> {
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
    address: &AbilityAddress,
) -> Result<ActivateCheckResult, Cow<'static, str>> {
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
    let source_card = ability_source::resolve(state, investigator, source)?;
    let source_code = source_card.code().clone();
    let source_exhausted = source_card.exhausted();

    // Invariant: `resolve_activated_ability` currently returns only `Ok(...)`
    // (success) or `Err(EngineOutcome::Rejected { ... })` (validation failure).
    // If a future PR extends it to return `AwaitingInput` (e.g. for an ability
    // requiring target selection during validation), this `unreachable!()` will
    // panic; the validator's caller chain in `activate_ability` would need to be
    // redesigned to thread the `AwaitingInput` outcome back through
    // `check_activate_ability`'s `Result` shape. Mirrors the same invariant
    // comment on `resolve_play_target` in `check_play_card`.
    let ActivatedAbility {
        action_cost,
        designator,
        costs,
        effect,
        usage_limit,
    } = match abilities::resolve_activated_ability(state, source, &source_code, address) {
        Ok(v) => v,
        Err(EngineOutcome::Rejected { reason }) => return Err(reason),
        Err(other) => {
            unreachable!("resolve_activated_ability returned non-Rejected outcome: {other:?}")
        }
    };
    reject_untrackable_usage_limit(source, &source_code, address, usage_limit)?;

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

    let (surcharge, surcharge_sources) =
        designated_action_surcharge(state, investigator, action_cost, designator.as_ref());
    let action_cost = action_cost.saturating_add(surcharge);

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

    reject_incompatible_costs(&costs)?;
    reject_source_costs_without_an_instance(source, &source_code, &costs)?;
    // Before the gate: a designated ability's change-state question is this
    // check's (`initiation::performs_an_action`).
    check_activation_target_available(state, investigator, designator.as_ref(), &effect)?;
    let candidate = ResolutionCandidate::new(
        source_code.clone(),
        investigator,
        address.clone(),
        CandidateSource::Ability(source),
    );
    initiation::check(state, &candidate, InitiationKind::Activated)
        .map_err(|refusal| activation_refusal(refusal, &source_code))?;

    Ok(ActivateCheckResult {
        source_code,
        action_cost,
        surcharge_sources,
        designator,
        costs,
        effect,
        usage_limit,
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
            )
            .maybe_at(a.target(cx.state))
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
    if card_registry::current().is_none() {
        return out;
    }
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
        for (source, code) in ability_source::reachable_source_codes(state, inv_id) {
            let Some(abilities) = abilities_in_effect::for_source(state, source, &code) else {
                continue;
            };
            for (address, ability) in abilities {
                let Trigger::Activated { action_cost: 0, .. } = ability.trigger else {
                    continue;
                };
                if check_activate_ability(state, inv_id, source, &address).is_ok() {
                    out.push(TurnAction::ActivateAbility {
                        investigator: inv_id,
                        source,
                        address,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
