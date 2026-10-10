//! Reaction-window and fast-window helpers.
//!
//! Contains the open/fire/close pipeline for reaction windows
//! ([`open_queued_reaction_window`], [`resume_reaction_window`],
//! [`fire_pending_trigger`], [`close_reaction_window`]) and the fast-window
//! machinery ([`any_fast_play_eligible`], [`enumerate_fast_plays`],
//! [`open_fast_window`]). Which reactions a window offers is the trigger
//! scan's, [`trigger_scan::collect_reactions`]; whether a card may be played or
//! an ability activated is [`legality`]'s, which the Fast-window enumeration
//! asks.

use card_dsl::dsl::{EventTiming, Trigger};

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::dispatch::emit::{ConditionResolution, TimingEvent};
use crate::engine::dispatch::initiation::{self, InitiationKind, Refusal};
use crate::engine::dispatch::legality::{check_activate_ability, check_play_card};
use crate::engine::dispatch::{cards, cursor, phases, skill_test, trigger_scan};
use crate::engine::enumerate::TurnAction;
use crate::engine::evaluator::EvalContext;
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use crate::engine::{abilities_in_effect, ability_source, Cx};
use crate::event::{Event, LapseReason};
use crate::state::{
    CandidateSource, Continuation, FastActorScope, FastWindowFrame, FastWindowKind, GameState,
    InvestigatorId, PlayFromHandFrame, ResolutionCandidate, TimingMode, TimingPointWindowFrame,
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
/// withdrawal *here* means [`trigger_scan::collect_reactions`] disagrees with itself over
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
/// reaction window uses, so every one records its cell, and both read it back
/// to re-validate before each prompt (#568, #607 — see
/// [`withdraw_lapsed_candidates`]).
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

/// Re-run the scan behind the open reaction window or forced run on top of the
/// stack and withdraw every candidate it no longer produces, emitting an
/// [`Event::ReactionOptionLapsed`] for each (#568, #607). Called at both prompt
/// sites, so the option list a player sees is never older than the board.
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
/// The scan that filled the frame — [`trigger_scan::collect_reactions`] for a
/// reaction window, [`trigger_scan::collect_forced`] for a forced run
/// ([`rescan`]) — *is* the definition of "may be offered here". Re-running it
/// and intersecting cannot drift from the gates the first scan applied, and
/// inherits any gate added later for free. The intersection
///
/// - **keeps multiplicity** — two copies of a card in hand are two candidates,
///   and one leaving hand withdraws exactly one of them;
/// - **never adds** — a card that entered play *during* the window was not in
///   play when the triggering condition occurred, so a fresh scan naming it is
///   not an invitation to offer it.
///
/// # A forced run is re-checked the same way (#607)
///
/// A 2+ lead-ordered run (#213) is a snapshot too, and resolving one of its
/// abilities can leave a later one with nothing to do. `glossary/Ability.md`:
///
/// > - If a forced ability does not have the potential to change the game
/// >   state, the ability does not initiate.
/// > - The initiation of a forced ability that has the potential to change the
/// >   game state is mandatory each time its specified timing point is met.
///
/// Mandatory, then, only while it can still change the game state — so the
/// run re-scans its cell with the forced collector before each prompt, exactly
/// as a reaction window does.
///
/// **A lapsed forced ability is withdrawn and logged, not silently skipped.**
/// The lead was shown it in an earlier prompt, so its disappearance is
/// observable; the [`Event::ReactionOptionLapsed`] says why, with the reason
/// the initiation gate gives when asked as [`InitiationKind::Forced`]. A run
/// emptied this way closes itself at both prompt sites, as a skipped reaction
/// window would — the run rejects `Skip`, so leaving it open with no options
/// would strand the lead at a mandatory prompt they cannot answer.
///
/// A [`FastWindow`](Continuation::FastWindow) is skipped: it has no reaction
/// candidates by construction ([`open_fast_window`] pushes an empty list) and no
/// timing cell to re-scan.
///
/// Returns how many candidates were withdrawn, which only [`open_reaction_run`]
/// reads (as a debug-only tripwire).
fn withdraw_lapsed_candidates(cx: &mut Cx) -> usize {
    let Some((event, bucket, mode)) = open_window_cell(cx.state) else {
        return 0;
    };
    let (event, bucket, mode) = (event.clone(), bucket, mode.clone());
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

    let mut fresh = rescan(cx.state, &event, bucket, &mode);
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
            reason: lapse_reason(cx.state, candidate, &mode),
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
/// happening. Reachable since #704 gave the enemy attack forced abilities of
/// its own — Dodge 01023's ruling is stated about a **Forced** ability, and
/// `crates/cards/tests/dodge.rs` proves it against Silver Twilight Acolyte
/// 01102. `0` for any other top frame, so the callers need no guard.
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

/// The `(event, cell, mode)` an open reaction window or forced run on top of
/// the stack was scanned at — the question a re-scan has to re-ask, and the
/// single place the "which frames are re-validated" test lives (#568, #607).
///
/// `None` for a [`FastWindow`](Continuation::FastWindow) and for every
/// non-window frame; the two callers turn that into their own no-op.
fn open_window_cell(state: &GameState) -> Option<(&TimingEvent, EventTiming, &TimingMode)> {
    match state.continuations.top() {
        Some(Continuation::TimingPointWindow(TimingPointWindowFrame {
            event,
            bucket,
            mode,
            ..
        })) => Some((event, *bucket, mode)),
        _ => None,
    }
}

/// The scan that filled a window of `mode`, re-run at its recorded cell: the
/// reaction scan for a reaction window, the forced collector for a forced run.
fn rescan(
    state: &GameState,
    event: &TimingEvent,
    bucket: EventTiming,
    mode: &TimingMode,
) -> Vec<ResolutionCandidate> {
    match mode {
        TimingMode::Reaction => trigger_scan::collect_reactions(state, event, bucket),
        TimingMode::Forced => trigger_scan::collect_forced(state, event, bucket),
    }
}

/// Why a withdrawn candidate lapsed, for the client log ([`LapseReason`],
/// #568). The withdrawal has already been decided by the re-scan in
/// [`withdraw_lapsed_candidates`]; this names the reason.
///
/// A source that is gone is [`LapseReason::SourceGone`]. Otherwise the
/// initiation gate is asked the question the scan asked of this candidate —
/// [`InitiationKind::Forced`] for an ability in a forced run,
/// [`InitiationKind::Play`] for a Fast event in hand, which is played, and
/// [`InitiationKind::Reaction`] for an ability source in a reaction window —
/// and its [`Refusal`] is the reason. A candidate the gate still passes dropped
/// out of the scan's own scoping instead, which the gate does not own:
/// [`LapseReason::OutOfScope`].
fn lapse_reason(
    state: &GameState,
    candidate: &ResolutionCandidate,
    mode: &TimingMode,
) -> LapseReason {
    if !candidate_source_present(state, candidate) {
        return LapseReason::SourceGone;
    }
    let kind = match (mode, candidate.source) {
        (TimingMode::Forced, _) => InitiationKind::Forced,
        (TimingMode::Reaction, CandidateSource::Hand) => InitiationKind::Play,
        (TimingMode::Reaction, CandidateSource::Ability(_)) => InitiationKind::Reaction,
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
/// or forced run's own timing cell — the single-candidate form of
/// [`withdraw_lapsed_candidates`], used as the fire-time gate in
/// [`fire_pending_trigger`] (#568, #607).
///
/// Membership, not multiplicity: the question is "may *this* option still be
/// initiated", and one surviving match answers it. `true` for any other top
/// frame — a [`FastWindow`] carries no reaction candidates to re-scan.
///
/// [`FastWindow`]: Continuation::FastWindow
fn candidate_still_offerable(state: &GameState, candidate: &ResolutionCandidate) -> bool {
    let Some((event, bucket, mode)) = open_window_cell(state) else {
        return true;
    };
    rescan(state, event, bucket, mode).contains(candidate)
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
    let (trigger, pending_idx, event) = {
        let window = cx
            .state
            .continuations
            .top()
            .expect("fire_pending_trigger: top frame is an open window/run");
        let candidates = window
            .pending_candidates()
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
        // The timing point the window is open at, which the fired effect binds
        // from. Only a `TimingPointWindow` holds candidates — a framework
        // `FastWindow`'s list is always empty, so its picks were rejected as out
        // of bounds above.
        let event = window
            .window_timing_event()
            .unwrap_or_else(|| {
                unreachable!(
                    "fire_pending_trigger: a window holding candidates is a timing-point \
                     window, which records its timing event"
                )
            })
            .clone();
        (candidates[idx].clone(), idx, event)
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
        return play_fast_event(cx, &trigger, &event);
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
    // Validate-first: this checks the ability still resolves before the window
    // is touched, so a rejected pick leaves the window as it was. `initiate`
    // resolves it again below, and nothing in between changes the answer.
    let code = trigger.code.clone();
    if abilities_in_effect::resolve(cx.state, trigger.source, &code, &trigger.address).is_none() {
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
    }

    // Drop the fired entry *before* resolving its effect: if the effect
    // suspends (a forced ability that initiates a skill test — Frozen in
    // Fear 01164), the entry must already be consumed so the resume drives
    // the *remaining* siblings, not this one again. The window is still the
    // top frame here (`initiate` pushes the effect above it).
    cx.state
        .continuations
        .top_frame_mut()
        .and_then(Continuation::pending_candidates_mut)
        .expect("fire_pending_trigger: top frame is an open window/run")
        .remove(pending_idx);

    // The one firing path a lone forced hit shares (#964): resolve, bind from
    // the window's timing event, record the use, push. The window frame beneath
    // stays with its remaining candidates, and `advance_resolution`
    // re-dispatches it once the effect (and any nested skill test) pops.
    if let Err(refusal) = initiation::initiate(cx, &trigger, &event) {
        unreachable!(
            "fire_pending_trigger: {code} resolved at {address:?} above and nothing has changed \
             since, yet initiate refused it ({refusal:?})",
            address = trigger.address,
        );
    }
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
fn play_fast_event(
    cx: &mut Cx,
    candidate: &ResolutionCandidate,
    event: &TimingEvent,
) -> EngineOutcome {
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
    // The bind-and-push tail a triggered ability's `initiate` ends on, so an
    // event naming "that enemy" or "that many" is bound as a reaction is (#964).
    initiation::push_bound_effect(cx, &effect, eval_ctx, event);
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
/// Returns `false` when the card registry isn't installed — same fallback as
/// [`trigger_scan::collect_reactions`].
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
