//! Reaction-window and fast-window helpers.
//!
//! Contains the open/scan/fire/close pipeline for after-event reaction
//! windows ([`queue_reaction_window`], [`scan_pending_triggers`],
//! [`trigger_matches`], [`open_queued_reaction_window`],
//! [`resume_reaction_window`], [`fire_pending_trigger`],
//! [`bump_usage_counter`], [`close_reaction_window_at`],
//! [`run_window_continuation`]) and the fast-window eligibility checks
//! ([`check_play_card`], [`check_activate_ability`],
//! [`any_fast_play_eligible`], [`open_fast_window`]).

use std::borrow::Cow;

use crate::action::InputResponse;
use crate::card_data::{CardMetadata, CardType};
use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger};
use crate::event::Event;
use crate::state::{
    CandidateSource, CardCode, CardInstanceId, Continuation, FastActorScope, FinishContinuation,
    ForcedContinuation, GameState, InvestigatorId, Phase, PhaseStep, ResolutionCandidate,
    ResolutionFrame, ResolutionKind, Status, WindowBinding, WindowKind,
};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use super::Cx;

/// Queue a reaction window of the given `kind` if any candidate matches —
/// an in-play card with a matching `Trigger::OnEvent` ability *or* (Axis C,
/// #335) a Fast event in hand whose play-instruction matches. No-op when the
/// registry isn't installed or nothing matches.
///
/// Emits [`Event::WindowOpened`] before pushing onto
/// [`GameState::open_windows`] so reaction-window observability is
/// symmetric with the Fast-window path ([`open_fast_window`]).
/// If no candidate matches the function returns early without
/// emitting anything — the window never opens.
///
/// The hand events are appended *after* the in-play triggers in the single
/// `pending_triggers` list, so they are offered as options after the
/// triggers; each carries [`CandidateSource::Hand`] so the fire path *plays*
/// it rather than firing an in-play ability.
///
/// The window suspends the surrounding driver
/// (today, [`drive_skill_test`]) at its next step boundary: after the
/// emit here the driver sees a non-empty `open_windows` stack and
/// returns [`EngineOutcome::AwaitingInput`] so the player can act.
///
/// Idempotency: if a window is already queued for this apply, the new
/// `kind` overwrites it. Phase-3 actions only emit one defeating
/// event per apply (a single Fight's `damage_enemy` call), so this case
/// doesn't arise; the overwrite is a loud-on-debug placeholder
/// rather than silent stacking — multi-window queueing lands when a
/// multi-defeat effect arrives.
pub(super) fn queue_reaction_window(cx: &mut Cx, kind: WindowKind) {
    let mut pending_triggers = scan_pending_triggers(cx.state, kind);
    // Axis C (#335): the window also opens for a matching Fast event in hand,
    // so a defeat with Evidence! in hand (and no in-play reaction) still opens
    // the after-defeat window. Hand plays are offered after the in-play
    // triggers.
    pending_triggers.extend(scan_hand_fast_events(cx.state, kind));
    if pending_triggers.is_empty() {
        return;
    }
    cx.events.push(Event::WindowOpened { kind });
    // Reaction windows admit any investigator's Fast actions
    // (Rules Reference: Fast may be played at any player window).
    // Multi-window nesting is now structural — push twice is valid.
    cx.state
        .continuations
        .push(Continuation::Resolution(ResolutionFrame {
            pending_triggers,
            kind: ResolutionKind::Window(WindowBinding {
                kind,
                fast_actors: FastActorScope::Any,
            }),
        }));
}

/// Open the forced-resolution run (Axis-B T5b / #213): push a
/// [`Forced`](ResolutionKind::Forced) [`ResolutionFrame`] holding the 2+
/// simultaneous forced `candidates`, and present the lead investigator's
/// order choice. The forced run is mandatory (cannot be skipped) and admits
/// no Fast plays. `continuation` names the framework flow to resume when the
/// run closes (see [`ForcedContinuation`]). The caller
/// ([`super::emit::emit_event`]) returns the resulting `AwaitingInput`.
pub(super) fn open_forced_resolution(
    cx: &mut Cx,
    candidates: Vec<ResolutionCandidate>,
    continuation: ForcedContinuation,
) -> EngineOutcome {
    cx.state
        .continuations
        .push(Continuation::Resolution(ResolutionFrame {
            pending_triggers: candidates,
            kind: ResolutionKind::Forced(continuation),
        }));
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

/// Scan every investigator's `cards_in_play` for
/// `Trigger::OnEvent` abilities matching `kind`, building a pending-
/// trigger list in active-investigator-first / turn-order resolution
/// order.
///
/// Returns an empty vec when the registry isn't installed (tests that
/// don't touch card data) or no cards match.
fn scan_pending_triggers(state: &GameState, kind: WindowKind) -> Vec<ResolutionCandidate> {
    let Some(reg) = card_registry::current() else {
        return Vec::new();
    };
    // Active investigator first, then the rest of turn_order in their
    // listed order. Investigators not in turn_order are skipped
    // entirely — the bare PerformSkillTest path can run without a
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
        // co-located with the attacked investigator. Other window kinds pass
        // all controllers through.
        if let WindowKind::BeforeEnemyAttack { investigator, .. } = kind {
            if !same_location(state, id, investigator) {
                continue;
            }
        }
        // "When YOU would discover … at YOUR location" (Cover Up 01007, Axis D
        // #336): the reaction's controller is the discoverer and must be at the
        // discovery location. (The per-card `clues > 0` potential gate is in the
        // card loop below.)
        if let WindowKind::BeforeDiscoverClues {
            investigator,
            location,
            ..
        } = kind
        {
            if id != investigator
                || state
                    .investigators
                    .get(&id)
                    .and_then(|i| i.current_location)
                    != Some(location)
            {
                continue;
            }
        }
        for card in inv.controlled_card_instances() {
            // Self-binding: for `AfterEnemyAttackDamagedAsset` only the
            // soaked asset instance may trigger `EnemyAttackDamagedSelf`.
            // All other instances are skipped here — the pattern match in
            // `trigger_matches` handles the pattern-kind pairing; this
            // filter enforces the "self = the soaked asset" scoping. (C5b
            // #237.) Other window kinds pass all instances through unchanged.
            if let WindowKind::AfterEnemyAttackDamagedAsset { asset, .. } = kind {
                if card.instance_id != asset {
                    continue;
                }
            }
            // Self-binding: `AfterEnteredPlay` fires only for the instance that
            // entered play (Research Librarian 01032). Mirrors the soaked-asset
            // filter above.
            if let WindowKind::AfterEnteredPlay { instance, .. } = kind {
                if card.instance_id != instance {
                    continue;
                }
            }
            // Potential-gate stand-in for Cover Up (RR p.2 "an ability cannot
            // initiate if its effect won't change the game state"; TODO(#368)):
            // only a source still holding clues to discard can replace the
            // discovery — an emptied Cover Up would otherwise prompt forever.
            if matches!(kind, WindowKind::BeforeDiscoverClues { .. }) && card.clues == 0 {
                continue;
            }
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (idx, ability) in abilities.iter().enumerate() {
                let Trigger::OnEvent {
                    pattern, timing, ..
                } = &ability.trigger
                else {
                    continue;
                };
                if !trigger_matches(kind, pattern, *timing, id) {
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
                // Reaction candidates always have a source instance (an
                // in-play / threat-area card); abilities resolve by `code`.
                pending.push(ResolutionCandidate {
                    code: card.code.clone(),
                    controller: id,
                    ability_index,
                    source: CandidateSource::InPlay(card.instance_id),
                });
            }
        }
    }
    pending
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
fn scan_hand_fast_events(state: &GameState, kind: WindowKind) -> Vec<ResolutionCandidate> {
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
        if let WindowKind::BeforeEnemyAttack { investigator, .. } = kind {
            if !same_location(state, id, investigator) {
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
                    pattern, timing, ..
                } = &ability.trigger
                else {
                    continue;
                };
                if !trigger_matches(kind, pattern, *timing, id) {
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
/// - [`WindowKind::AfterEnemyDefeated`] matches
///   [`EventPattern::EnemyDefeated`] with
///   [`EventTiming::After`]. The `by_controller` qualifier narrows to
///   defeats credited to this ability's controller.
///
/// `EventTiming::Before` doesn't fire on these windows yet — the
/// "Forced — when X would Y" timing needs a separate pre-event
/// scanning hook when the first such card lands.
fn trigger_matches(
    kind: WindowKind,
    pattern: &EventPattern,
    timing: EventTiming,
    controller: InvestigatorId,
) -> bool {
    // Before-timing windows fire only for their exact pattern pairing (Axis D
    // #336); the "at your location" / eligibility scoping lives in the scans.
    match timing {
        EventTiming::Before => {
            return matches!(
                (kind, pattern),
                (
                    WindowKind::BeforeEnemyAttack { .. },
                    EventPattern::EnemyAttacks
                ) | (
                    WindowKind::BeforeDiscoverClues { .. },
                    EventPattern::WouldDiscoverClues
                )
            );
        }
        EventTiming::After => {}
    }
    match (kind, pattern) {
        (
            WindowKind::AfterEnemyDefeated { by, .. },
            EventPattern::EnemyDefeated {
                by_controller,
                code: _,
            },
        ) => {
            if *by_controller {
                by == Some(controller)
            } else {
                true
            }
        }
        // `AfterEnemyAttackDamagedAsset` matches `EnemyAttackDamagedSelf`
        // only. The soaked-asset self-binding is enforced by the instance
        // filter in `scan_pending_triggers` (only the `asset` instance
        // reaches `trigger_matches` for this window kind). Sole consumer:
        // Guard Dog 01021's "deal 1 damage to the attacking enemy"
        // reaction. (C5b #237.)
        (WindowKind::AfterEnemyAttackDamagedAsset { .. }, EventPattern::EnemyAttackDamagedSelf) => {
            true
        }
        // `AfterSuccessfulInvestigate` matches `SuccessfullyInvestigated`,
        // scoped to the controller's own investigation ("after **you**
        // investigate" — Dr. Milan 01033). (C6a #241.)
        (
            WindowKind::AfterSuccessfulInvestigate { investigator },
            EventPattern::SuccessfullyInvestigated,
        ) => investigator == controller,
        // `AfterEnteredPlay` matches `EnteredPlay`, scoped to the controller
        // (the entered card's owner). The self-instance scoping is in the scan
        // (`scan_pending_triggers` filters to the entered instance).
        (
            WindowKind::AfterEnteredPlay {
                controller: window_controller,
                ..
            },
            EventPattern::EnteredPlay,
        ) => window_controller == controller,
        // PlayerWindow steps open for timing reasons; no
        // Trigger::OnEvent pattern matches them — those windows gate
        // Fast actions, not after-event reactions. AfterEnemyDefeated
        // windows only match EnemyDefeated patterns (handled above);
        // encounter-reveal patterns return false.
        //
        // EnemySpawned: no WindowKind opens specifically for "enemy
        // spawned" in Phase 4. A future PR (likely Phase-7+) that wants
        // to react to spawns will add the corresponding WindowKind
        // variant and update this arm.
        // EnteredLocation is matched by the forced auto-fire path in
        // `engine::dispatch::forced_triggers` (fired from `move_action`),
        // not by reaction windows.
        // PhaseEnded is matched only by the forced dispatch path
        // (`engine::dispatch::forced_triggers`), never by player reaction
        // windows.
        // ActAdvanced is matched only by the forced dispatch path
        // (`ForcedTriggerPoint::ActAdvanced`), never by player reaction
        // windows.
        // EndOfTurn and AfterLocationInvestigated are likewise forced-only
        // (`ForcedTriggerPoint::EndOfTurn` / `AfterLocationInvestigated`).
        // WouldDiscoverClues is matched only by the `discover_clue`
        // interrupt seam, and GameEnd only by `ForcedTriggerPoint::GameEnd`
        // — both seam/forced-only, never player windows (C5a #236).
        // `AfterSuccessfulInvestigate` matches only `SuccessfullyInvestigated`
        // (handled above); `AfterLocationInvestigated` is the forced twin,
        // never matched by a reaction window.
        // The Before-timing window kinds never reach here (the `Before`
        // branch returned above); listed for exhaustiveness, and the
        // `EnemyAttacks` pattern is likewise Before-only (Axis D #336).
        (
            WindowKind::PlayerWindow(_)
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::AfterEnemyAttackDamagedAsset { .. }
            | WindowKind::AfterSuccessfulInvestigate { .. }
            | WindowKind::AfterEnteredPlay { .. }
            | WindowKind::BeforeEnemyAttack { .. }
            | WindowKind::BeforeDiscoverClues { .. },
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned
            | EventPattern::EnteredLocation
            | EventPattern::PhaseEnded { .. }
            | EventPattern::ActAdvanced
            | EventPattern::AgendaAdvanced
            | EventPattern::RoundEnded
            | EventPattern::EndOfTurn
            | EventPattern::AfterLocationInvestigated
            | EventPattern::WouldDiscoverClues
            | EventPattern::GameEnd
            | EventPattern::EnemyAttackDamagedSelf
            | EventPattern::SuccessfullyInvestigated
            | EventPattern::EnemyAttacks
            | EventPattern::EnteredPlay
            | EventPattern::LeftLocation,
        ) => false,
    }
}

/// Build the structured option list for a resolution frame: one
/// [`ChoiceOption`] per pending candidate, in `pending_triggers` order.
/// `OptionId(i)` is the index into the returned list — the Axis-A convention
/// shared with [`super::choice`]. The label distinguishes a hand Fast-event
/// play ([`CandidateSource::Hand`]) from an in-play reaction.
fn build_resolution_options(frame: &ResolutionFrame) -> Vec<ChoiceOption> {
    frame
        .pending_triggers
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
/// the top of [`GameState::open_windows`]. Called by [`drive_skill_test`]
/// at a step boundary when an earlier step queued a window via
/// [`queue_reaction_window`].
///
/// [`Event::WindowOpened`] is emitted by [`queue_reaction_window`]
/// (not here) so the event appears at queue time and is symmetric with
/// the [`open_fast_window`] path.
pub(crate) fn open_queued_reaction_window(cx: &mut Cx) -> EngineOutcome {
    let window = cx
        .state
        .top_reaction_window()
        .expect("open_queued_reaction_window: caller checked is_some");
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip; the lead orders them)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let options = build_resolution_options(window);
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(
            format!(
                "Resolution window: {} option(s). \
                 Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
                options.len(),
            ),
            options,
        ),
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
/// Closing the window emits [`Event::WindowClosed`] with the same
/// kind, pops the top entry from [`GameState::open_windows`], and
/// returns [`Done`].
pub(super) fn resume_reaction_window(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        // `OptionId(i)` indexes the single `pending_triggers` list (see
        // `build_resolution_options`); `fire_pending_trigger` dispatches on
        // the candidate's source (in-play ability vs. Axis-C hand play).
        InputResponse::PickSingle(OptionId(i)) => fire_pending_trigger(cx, *i),
        InputResponse::Skip => {
            // Resolve the active reaction-window index up-front so the
            // close path operates on the same window the driver had
            // been driving (not the absolute top of `open_windows`,
            // which may be an empty-pending_triggers window sitting
            // above it).
            let idx = cx
                .state
                .top_reaction_window_index()
                .expect("resume_reaction_window: caller checked is_some");
            // Forced abilities are mandatory — the forced run (`window: None`)
            // cannot be skipped (RR p.2 / #213). The lead must pick one.
            if cx.state.continuations[idx]
                .as_resolution()
                .is_some_and(ResolutionFrame::is_forced)
            {
                return EngineOutcome::Rejected {
                    reason: "ResolveInput::Skip: forced abilities are mandatory; submit \
                             InputResponse::PickSingle(OptionId) to resolve one (the lead \
                             orders them)"
                        .into(),
                };
            }
            close_reaction_window_at(cx, idx)
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
    // Capture the index of the reaction window we're driving up-front
    // so the close path removes the same entry (not the absolute top of
    // the stack, which may be a different, empty-pending_triggers
    // window once non-reaction windows can sit above one).
    let window_idx = cx
        .state
        .top_reaction_window_index()
        .expect("fire_pending_trigger: caller checked is_some");
    // Snapshot to avoid borrowing state across the apply_effect call.
    let (trigger, pending_idx) = {
        let window = cx.state.continuations[window_idx]
            .as_resolution()
            .expect("fire_pending_trigger: top_reaction_window_index points at a Resolution frame");
        let idx = match usize::try_from(i) {
            Ok(idx) if idx < window.pending_triggers.len() => idx,
            _ => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: reaction-window PickSingle(OptionId({i})) out of bounds \
                         (pending size {})",
                        window.pending_triggers.len(),
                    )
                    .into(),
                };
            }
        };
        (window.pending_triggers[idx].clone(), idx)
    };

    // Axis C (#335): a hand candidate is *played*, not fired in place. Remove
    // it from the run first (so a suspending play resumes the remaining
    // siblings, not this one again — mirrors the in-play path below), then
    // play it.
    if trigger.source == CandidateSource::Hand {
        {
            let window = cx.state.continuations[window_idx]
                .as_resolution_mut()
                .expect("fire_pending_trigger: window_idx is a Resolution frame");
            window.pending_triggers.remove(pending_idx);
        }
        return play_fast_event(cx, window_idx, &trigger);
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
    match cx.state.continuations[window_idx]
        .as_resolution()
        .and_then(ResolutionFrame::kind)
    {
        Some(WindowKind::AfterEnemyAttackDamagedAsset { enemy, .. }) => {
            eval_ctx.set_attacking_enemy(enemy);
        }
        // For `BeforeDiscoverClues`, bind the would-be discovery count so the
        // replacement effect (Cover Up's "discard that many") discards the
        // right number. Mirrors `attacking_enemy`. TODO(#368): `count` is the
        // requested, not the capped, count.
        Some(WindowKind::BeforeDiscoverClues { count, .. }) => {
            eval_ctx.set_clue_discovery_count(count);
        }
        _ => {}
    }
    let usage_limit = ability.usage_limit;

    // Drop the fired entry *before* resolving its effect: if the effect
    // suspends (a forced ability that initiates a skill test — Frozen in
    // Fear 01164), the entry must already be consumed so the resume drives
    // the *remaining* siblings, not this one again. `apply_effect` does not
    // push/pop continuations, so `window_idx` stays valid across it.
    {
        let window = cx.state.continuations[window_idx]
            .as_resolution_mut()
            .expect("fire_pending_trigger: window_idx is a Resolution frame");
        window.pending_triggers.remove(pending_idx);
    }

    let result = apply_effect(cx, &ability.effect, eval_ctx);
    match result {
        EngineOutcome::Rejected { reason } => {
            // Card-impl bugs surface loudly — same policy as
            // `fire_on_skill_test_resolution`.
            unreachable!(
                "OnEvent reaction: effect for card {code:?} rejected unexpectedly: {reason}"
            );
        }
        // The effect suspended (it started a skill test, pushing a `SkillTest`
        // frame above this run's frame). Park: the run's frame stays with its
        // remaining siblings, and `advance_resolution` re-enters it once the
        // nested test resolves (Axis-B T5b reentrancy). In-scope suspending
        // forced effects (Frozen in Fear) carry no usage limit, so skipping
        // the bump here is correct for the current pool.
        suspended @ EngineOutcome::AwaitingInput { .. } => suspended,
        EngineOutcome::Done => {
            if usage_limit.is_some() {
                bump_usage_counter(cx.state, &trigger);
            }
            advance_resolution(cx, window_idx)
        }
    }
}

/// Play the hand Fast-event `candidate` from the resolution run at
/// `window_idx` (Axis C, #335) — the [`CandidateSource::Hand`] resolution of
/// [`fire_pending_trigger`]. Commences the play via the shared
/// [`super::cards::begin_event_play`] (emit [`Event::CardPlayed`], leave hand,
/// stash in [`GameState::pending_played_event`] — RR Appendix I step 3), runs
/// the matched `OnEvent` ability's effect, then advances the run. The apply
/// loop flushes the event to discard on completion (step 4) — the
/// suspending-event path Dynamite Blast 01024 uses.
///
/// Charges no resource cost, matching [`super::cards::play_card`] (Slice 1
/// does not model play-cost resources). The caller has already removed the
/// candidate from the run, so a suspending effect's resume drives the
/// remaining siblings, not this play again.
fn play_fast_event(
    cx: &mut Cx,
    window_idx: usize,
    candidate: &ResolutionCandidate,
) -> EngineOutcome {
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

    // Run the matched OnEvent ability's effect under the playing investigator.
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

    match apply_effect(cx, &effect, eval_ctx) {
        EngineOutcome::Rejected { reason } => unreachable!(
            "Fast-event play: effect for {:?} rejected unexpectedly: {reason}",
            candidate.code,
        ),
        suspended @ EngineOutcome::AwaitingInput { .. } => suspended,
        EngineOutcome::Done => {
            // The Fast event's effect completed (RR Appendix I step 4): discard
            // it NOW, before advancing the window / phase cascade. The apply
            // loop's `flush_pending_played_event` only fires on a `Done` apply,
            // so deferring to it would strand the event in `pending_played_event`
            // whenever this same apply later suspends for an unrelated reason —
            // e.g. the window close cascades into the Mythos 1.4 draw prompt
            // (#348) or an upkeep hand-size discard (#111), both `AwaitingInput`.
            // A no-op if the effect already disposed of the card (e.g. an event
            // that becomes an asset clears `pending_played_event` itself).
            super::cards::flush_pending_played_event(cx);
            advance_resolution(cx, window_idx)
        }
    }
}

/// Advance a resolution run after one of its candidates resolved: close it
/// (running its continuation) when none remain, else re-emit the pick prompt.
///
/// Shared by [`fire_pending_trigger`]'s synchronous tail and the skill-test
/// commit-resume path ([`super::resume_skill_test_commit`]) — the latter
/// re-enters a forced run parked beneath a sibling's now-resolved skill test.
pub(super) fn advance_resolution(cx: &mut Cx, window_idx: usize) -> EngineOutcome {
    let window = cx.state.continuations[window_idx]
        .as_resolution()
        .expect("advance_resolution: window_idx is a Resolution frame");
    // Close when no candidate remains. Hand Fast-event plays (Axis C) ride
    // `pending_triggers` alongside in-play triggers, so this single check
    // keeps a window with only a remaining hand play open.
    if window.pending_triggers.is_empty() {
        return close_reaction_window_at(cx, window_idx);
    }
    let skip_hint = if window.is_forced() {
        " (forced — cannot skip)"
    } else {
        ", or InputResponse::Skip to close"
    };
    let options = build_resolution_options(window);
    EngineOutcome::AwaitingInput {
        request: InputRequest::choice(
            format!(
                "Resolution window: {} option(s). \
                 Submit InputResponse::PickSingle(OptionId) to resolve one{skip_hint}.",
                options.len(),
            ),
            options,
        ),
        resume_token: ResumeToken(0),
    }
}

/// Bump the per-instance ability-usage counter for the just-fired
/// trigger. Called by [`fire_pending_trigger`] only for abilities
/// whose `usage_limit` is `Some(_)`; for abilities with no limit
/// nothing tracks them.
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
    // Only usage-limited abilities reach here, and those are on in-play
    // instances (reactions) — so the source is always `InPlay`.
    let CandidateSource::InPlay(instance_id) = trigger.source else {
        unreachable!(
            "bump_usage_counter: a usage-limited candidate must be an in-play instance \
             (board / hand candidates carry no usage limits); candidate {trigger:?}"
        )
    };
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
    let card = inv
        .cards_in_play
        .iter_mut()
        .chain(inv.threat_area.iter_mut())
        .find(|c| c.instance_id == instance_id)
        .unwrap_or_else(|| {
            unreachable!(
                "bump_usage_counter: instance {instance_id:?} vanished from controller {ctl:?}'s \
                 cards_in_play / threat area while reaction window was open; \
                 state-corruption invariant violation",
                ctl = trigger.controller,
            )
        });
    card.bump_ability_usage(trigger.ability_index, current_round);
}

/// Close the reaction window at `idx` in [`GameState::open_windows`].
/// Rejects when any forced trigger is still pending (player must fire
/// them first). On success emits [`Event::WindowClosed`], removes the
/// window at the specified index (not necessarily the top of the
/// stack), and either resumes a paused skill-test driver (if one was
/// mid-resolution when the window opened) or returns [`Done`].
///
/// # Why an explicit index
///
/// `top_reaction_window_mut` skips empty-`pending_triggers` windows
/// when finding the active reaction window. The close path must
/// remove the same window the driver operated on, not the absolute
/// top of the stack — once a `PlayerWindow` gate (or any other
/// non-reaction window) can sit above an active reaction window
/// (#69/#70/#71), a naive `open_windows.pop()` would remove the wrong
/// entry. Callers compute the index via
/// [`GameState::top_reaction_window_index`].
pub(super) fn close_reaction_window_at(cx: &mut Cx, idx: usize) -> EngineOutcome {
    // Reaction windows are all-optional, so `Skip` always closes them. The
    // "forced abilities are mandatory" rule lives in the forced resolution
    // run (its frame is `window: None` — Axis-B T5b), not here.
    let removed = cx.state.continuations.remove(idx);
    let frame = removed
        .as_resolution()
        .expect("close_reaction_window_at: removed frame must be a Resolution frame");

    // A window emits `WindowClosed` and runs its kind-specific continuation
    // (e.g. MythosAfterDraws → mythos_phase_end). The forced run (#213) is
    // not a window: no event; instead it resumes the framework flow it
    // suspended via its `ForcedContinuation`. Either may suspend (e.g. the
    // upkeep act round-end advance window), so propagate the outcome.
    let continuation = if let Some(kind) = frame.kind() {
        cx.events.push(Event::WindowClosed { kind });
        run_window_continuation(cx, kind)
    } else {
        let cont = frame.forced_continuation().unwrap_or_else(|| {
            unreachable!(
                "close_reaction_window_at: a non-window Resolution frame is the forced \
                 run and must carry a ForcedContinuation"
            )
        });
        resume_forced_continuation(cx, cont)
    };
    if matches!(continuation, EngineOutcome::AwaitingInput { .. }) {
        return continuation;
    }
    debug_assert!(
        matches!(continuation, EngineOutcome::Done),
        "close_reaction_window_at: window continuation returned unexpected {continuation:?} \
         (expected Done or AwaitingInput)",
    );

    // If a skill test was mid-resolution when this window opened,
    // hand control back to its driver to run the remaining steps.
    // `AwaitingCommit` means the test is parked at the commit
    // window (no driver state to resume); this happens when a future
    // non-skill-test action queues a window — `Done` is the right
    // terminal outcome.
    if let Some(in_flight) = cx.state.current_skill_test() {
        if !matches!(in_flight.continuation, FinishContinuation::AwaitingCommit) {
            return super::skill_test::drive_skill_test(cx);
        }
    }

    EngineOutcome::Done
}

/// Kind-aware continuation called when a window closes (whether inline via
/// [`open_fast_window`]'s auto-skip path or via the [`close_reaction_window_at`]
/// pop path).
///
/// Every framework [`WindowKind::PlayerWindow`] close routes to the `*Phase`
/// anchor beneath it via
/// [`anchor_on_child_pop`](super::phases::anchor_on_child_pop) (slice 1a, #393):
/// the anchor's `resume` — not the [`PhaseStep`] — selects the relocated body
/// (the Mythos/Investigation transitions, the Enemy attack-loop step incl. its
/// mid-loop soak-window `AwaitingInput` propagation, the Upkeep 4.2–4.6
/// cascade). The card/ability-reaction kinds run inline here: soak / before-
/// attack ([`WindowKind::AfterEnemyAttackDamagedAsset`] /
/// [`WindowKind::BeforeEnemyAttack`]) re-enter the attack loop via
/// [`resume_enemy_attack`](super::combat::resume_enemy_attack);
/// [`WindowKind::BeforeDiscoverClues`] performs the deferred discovery;
/// [`WindowKind::AfterEnemyDefeated`] / [`WindowKind::AfterSuccessfulInvestigate`]
/// / [`WindowKind::AfterEnteredPlay`] have no continuation work.
///
/// Returns the continuation's [`EngineOutcome`] — `Done`, or `AwaitingInput`
/// when a body suspends (an Enemy soak window, the Upkeep step-4.5 hand-size
/// discard, …).
pub(super) fn run_window_continuation(cx: &mut Cx, kind: WindowKind) -> EngineOutcome {
    match kind {
        // Every framework `PlayerWindow(PhaseStep)` close routes to the `*Phase`
        // anchor beneath it (slice 1a, #393): the anchor's `resume` selects the
        // relocated body — the skill-test-in-flight guards, the Enemy
        // soak-window `AwaitingInput` propagation, the Upkeep 4.2–4.6 cascade,
        // the Mythos/Investigation transitions. The `PhaseStep` is no longer the
        // continuation key; the anchor's `resume` is.
        WindowKind::PlayerWindow(_) => super::phases::anchor_on_child_pop(cx),
        // AfterEnemyDefeated / AfterSuccessfulInvestigate: no continuation
        // work. The skill-test driver (which queued the window mid-resolution)
        // resumes via `close_reaction_window_at`'s in-flight re-entry.
        WindowKind::AfterEnemyDefeated { .. } | WindowKind::AfterSuccessfulInvestigate { .. } => {
            EngineOutcome::Done
        }
        // AfterEnteredPlay (Research Librarian 01032): no continuation work —
        // the asset entered play before the window opened, so closing the
        // window just finishes the play action.
        WindowKind::AfterEnteredPlay { .. } => EngineOutcome::Done,
        // AfterEnemyAttackDamagedAsset (soak, C5b #237) + BeforeEnemyAttack
        // (cancel, Axis D #336): re-enter the enemy-attack loop the window
        // suspended. `resume_enemy_attack` reads its parked phase to either
        // honor the cancel + deal the head attacker (BeforeAttack) or drain
        // the remaining attackers (AfterSoak), then for the enemy phase runs
        // `after_enemy_phase_attacks` once the loop finishes.
        WindowKind::AfterEnemyAttackDamagedAsset { .. } | WindowKind::BeforeEnemyAttack { .. } => {
            super::combat::resume_enemy_attack(cx)
        }
        // BeforeDiscoverClues (Cover Up 01007, Axis D #336): the before-discover
        // window closed. If a reaction cancelled the discovery (Cover Up played
        // its `Seq[discard, Cancel]`), skip it; otherwise perform the deferred
        // discovery. Then, if a skill test is in flight (the dominant path:
        // Investigate's follow-up discovery), re-enter its driver — its
        // continuation was pre-advanced to `PostFollowUp` by `finish_skill_test`
        // before the follow-up suspended, so this picks up at teardown.
        WindowKind::BeforeDiscoverClues {
            investigator,
            location,
            count,
        } => resume_before_discover_window(cx, investigator, location, count),
    }
}

/// Resume after a [`WindowKind::BeforeDiscoverClues`] window closes (Cover Up
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
    if cx.state.has_skill_test_in_flight() {
        super::skill_test::drive_skill_test(cx)
    } else {
        EngineOutcome::Done
    }
}

/// Resume the framework flow a closed forced run (#213) suspended.
///
/// The forced run opens only when 2+ simultaneous forced abilities fire at a
/// timing point and the lead must order them. Once they all resolve, control
/// returns here to run whatever framework work followed the emit site, named
/// by the [`ForcedContinuation`] the run carried (see
/// [`super::emit::TimingEvent`]'s `forced_continuation`). May itself suspend
/// (the upkeep tail opens the act round-end advance window), so propagate.
fn resume_forced_continuation(cx: &mut Cx, continuation: ForcedContinuation) -> EngineOutcome {
    match continuation {
        // Genuinely terminal emit site (e.g. a move's "when you enter"
        // forced abilities) — nothing follows.
        ForcedContinuation::Terminal => EngineOutcome::Done,
        // "Upkeep phase ends. Round ends." — run the upkeep teardown
        // (expire until-end-of-round effects, then Upkeep→Mythos). The act's
        // `when the round ends` window already resolved before the `at` forced
        // run that scheduled this continuation.
        ForcedContinuation::UpkeepAfterRoundEnded => super::phases::upkeep_round_end_teardown(cx),
        // End of turn — run the end-of-turn tail (rotate to the next active
        // investigator, or end the Investigation phase).
        ForcedContinuation::EndOfTurnAfterForced { investigator } => {
            super::phases::resume_end_turn(cx, investigator)
        }
    }
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

    if let Some(inv) = next {
        super::phases::set_enemy_anchor(
            cx,
            crate::state::EnemyResume::BeforeInvestigatorAttacked,
            Some(inv),
        );
        open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::BeforeInvestigatorAttacked),
        )
    } else {
        super::phases::set_enemy_anchor(cx, crate::state::EnemyResume::AfterAllAttacked, None);
        open_fast_window(
            cx,
            WindowKind::PlayerWindow(PhaseStep::AfterAllInvestigatorsAttacked),
        )
    }
}

/// Open a printed Fast-play window of the given kind. Always emits
/// [`Event::WindowOpened`] for observability. Then either:
///
/// - Pushes the [`ResolutionFrame`] onto [`GameState::open_windows`] if any
///   pending reaction triggers or Fast-eligible plays are detected. The
///   apply loop's existing "pending reactions → `AwaitingInput`" path
///   then surfaces the wait at the dispatch tail.
/// - Or emits [`Event::WindowClosed`] immediately, pops the transiently
///   pushed window, and runs [`run_window_continuation`] inline. This
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
pub(super) fn open_fast_window(cx: &mut Cx, kind: WindowKind) -> EngineOutcome {
    cx.events.push(Event::WindowOpened { kind });

    // Push first so any_fast_play_eligible's check_play_card call sees
    // this window in state.open_windows when evaluating permissive_window.
    let pending_triggers = scan_pending_triggers(cx.state, kind);
    cx.state
        .continuations
        .push(Continuation::Resolution(ResolutionFrame {
            pending_triggers,
            kind: ResolutionKind::Window(WindowBinding {
                kind,
                fast_actors: FastActorScope::Any,
            }),
        }));

    let has_pending = !cx
        .state
        .top_window()
        .expect("just pushed; cannot be empty")
        .pending_triggers
        .is_empty();
    let has_fast_eligible = any_fast_play_eligible(cx.state);

    if !has_pending && !has_fast_eligible {
        // Auto-skip: nothing to do. Pop the window we just pushed,
        // emit WindowClosed, and run the continuation inline, so the
        // net effect on the continuation stack is the same as before.
        let _ = cx.state.continuations.pop();
        cx.events.push(Event::WindowClosed { kind });
        return run_window_continuation(cx, kind);
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
        .is_some_and(|w| w.fast_actors().is_some_and(|fa| fa.permits(investigator)));
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
    Ok(super::PlayCheckResult {
        destination,
        abilities,
        is_fast,
        card_type,
    })
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
/// - **Fight:** needs exactly one engaged enemy (0 = no target; 2+ multi-target
///   selection deferred to #212/#213).
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
        && super::combat::single_engaged_enemy(state, investigator).is_none()
    {
        return Err(
            "ActivateAbility: a Fight ability needs exactly one engaged enemy \
             (0 = no target; 2+ multi-target selection deferred with #212/#213)"
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
        .is_some_and(|w| w.fast_actors().is_some_and(|fa| fa.permits(investigator)));
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
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    for (&inv_id, inv) in &state.investigators {
        // Fast events / Fast assets in hand.
        for hand_idx_usize in 0..inv.hand.len() {
            let Ok(hand_idx) = u8::try_from(hand_idx_usize) else {
                break;
            };
            if let Ok(result) = check_play_card(state, inv_id, hand_idx) {
                if result.is_fast {
                    return true;
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
                let Ok(ab_idx_u8) = u8::try_from(ab_idx) else {
                    break;
                };
                if check_activate_ability(state, inv_id, card.instance_id, ab_idx_u8).is_ok() {
                    return true;
                }
            }
        }
    }
    false
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
    use crate::state::{EnemyId, PhaseStep};

    #[test]
    fn would_discover_clues_never_matches_a_player_window() {
        // The before-timing clue-discovery interrupt (Cover Up 01007) is
        // matched only by the `discover_clue` seam, never a player reaction
        // window — even with After timing. (C5a #236.)
        assert!(!trigger_matches(
            WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
            &EventPattern::WouldDiscoverClues,
            EventTiming::After,
            InvestigatorId(1),
        ));
    }

    #[test]
    fn trigger_matches_before_pairs() {
        use crate::state::LocationId;
        let inv = InvestigatorId(1);
        assert!(trigger_matches(
            WindowKind::BeforeEnemyAttack {
                enemy: EnemyId(1),
                investigator: inv
            },
            &EventPattern::EnemyAttacks,
            EventTiming::Before,
            inv,
        ));
        assert!(trigger_matches(
            WindowKind::BeforeDiscoverClues {
                investigator: inv,
                location: LocationId(2),
                count: 1
            },
            &EventPattern::WouldDiscoverClues,
            EventTiming::Before,
            inv,
        ));
        // Wrong timing for the pairing → no match.
        assert!(!trigger_matches(
            WindowKind::BeforeEnemyAttack {
                enemy: EnemyId(1),
                investigator: inv
            },
            &EventPattern::EnemyAttacks,
            EventTiming::After,
            inv,
        ));
        // A Before window only matches its own pattern.
        assert!(!trigger_matches(
            WindowKind::BeforeEnemyAttack {
                enemy: EnemyId(1),
                investigator: inv
            },
            &EventPattern::WouldDiscoverClues,
            EventTiming::Before,
            inv,
        ));
    }

    #[test]
    fn game_end_never_matches_a_player_window() {
        // GameEnd is forced-only (`ForcedTriggerPoint::GameEnd`).
        assert!(!trigger_matches(
            WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
            &EventPattern::GameEnd,
            EventTiming::After,
            InvestigatorId(1),
        ));
    }

    /// `soak_window_matches_only_self_instance` — direct `trigger_matches`
    /// coverage for the `AfterEnemyAttackDamagedAsset` + `EnemyAttackDamagedSelf`
    /// true arm added in Task 8 (C5b #237).
    ///
    /// The instance-level scoping (only the soaked `asset` instance fires, not
    /// every controlled card) is enforced by the filter in `scan_pending_triggers`
    /// one layer up; that filter is exercised end-to-end in the EU5 Guard Dog
    /// integration test (`crates/cards/tests/guard_dog_soak.rs`), which installs
    /// the real `cards::REGISTRY` in an isolated process. Testing it here would
    /// require a global `card_registry::install`, which is `OnceLock`-guarded
    /// and cannot be reset between tests. So this unit test asserts the
    /// `trigger_matches` contract directly:
    ///  - `AfterEnemyAttackDamagedAsset` + `EnemyAttackDamagedSelf` → `true`
    ///  - `AfterEnemyAttackDamagedAsset` + any other pattern → `false`
    #[test]
    fn soak_window_matches_only_self_instance() {
        let asset = CardInstanceId(7);
        let enemy = EnemyId(1);
        let controller = InvestigatorId(1);
        let kind = WindowKind::AfterEnemyAttackDamagedAsset {
            asset,
            enemy,
            controller,
        };

        // The soak-self pattern must match the soak window. (C5b #237.)
        assert!(
            trigger_matches(
                kind,
                &EventPattern::EnemyAttackDamagedSelf,
                EventTiming::After,
                controller
            ),
            "AfterEnemyAttackDamagedAsset must match EnemyAttackDamagedSelf"
        );
        // No other pattern matches this window kind.
        assert!(
            !trigger_matches(
                kind,
                &EventPattern::EnemyDefeated {
                    by_controller: false,
                    code: None
                },
                EventTiming::After,
                controller
            ),
            "AfterEnemyAttackDamagedAsset must not match EnemyDefeated"
        );
        assert!(
            !trigger_matches(
                kind,
                &EventPattern::EnemyAttackDamagedSelf,
                EventTiming::Before,
                controller
            ),
            "Before timing must never match this After-only window/pattern pair"
        );
        // The soak-self pattern must NOT match any other window kind — guards
        // the match-arm ordering (the `=> true` arm is scoped to this kind;
        // these pairings must fall through to the `false` catch-all). (C5b #237.)
        for other_kind in [
            WindowKind::PlayerWindow(PhaseStep::InvestigatorTurnBegins),
            WindowKind::AfterEnemyDefeated {
                enemy,
                by: Some(controller),
            },
        ] {
            assert!(
                !trigger_matches(
                    other_kind,
                    &EventPattern::EnemyAttackDamagedSelf,
                    EventTiming::After,
                    controller
                ),
                "{other_kind:?} must not match EnemyAttackDamagedSelf"
            );
        }
        // Instance-filter (only the keyed `asset` instance fires, not every
        // controlled card) is asserted in the EU5 Guard Dog integration test
        // (`crates/cards/tests/guard_dog_soak.rs`) which can install the real
        // registry. The `scan_pending_triggers` `continue` on `instance_id !=
        // asset` is the load-bearing line; grep it if this note becomes stale.
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
    use crate::event::Event;
    use crate::state::{EnemyId, WindowKind};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn open_fast_window_with_no_eligibility_emits_open_then_close_inline() {
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
            WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws),
        );

        assert!(
            state.open_windows().is_empty(),
            "auto-skip must not leave the window on the stack"
        );
        assert!(
            matches!(
                events.first(),
                Some(Event::WindowOpened {
                    kind: WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws)
                })
            ),
            "first event must be WindowOpened; got {:?}",
            events.first()
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::PlayerWindow(PhaseStep::MythosAfterDraws)
                }
            )),
            "must emit WindowClosed for MythosAfterDraws; events = {events:?}"
        );
    }

    #[test]
    fn run_window_continuation_for_no_continuation_kind_does_nothing() {
        // AfterEnemyDefeated has no continuation. Calling it must be a
        // no-op (no events, no state change).
        let mut state = GameStateBuilder::default().build();
        let mut events = Vec::new();
        let result = run_window_continuation(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            WindowKind::AfterEnemyDefeated {
                enemy: EnemyId(1),
                by: None,
            },
        );
        assert_eq!(result, EngineOutcome::Done);
        assert!(
            events.is_empty(),
            "AfterEnemyDefeated continuation must be a no-op; events = {events:?}"
        );
    }
}
