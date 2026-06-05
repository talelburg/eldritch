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
use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::{EventPattern, EventTiming, Trigger};
use crate::event::Event;
use crate::state::{
    CardCode, CardInstanceId, FastActorScope, FinishContinuation, GameState, InvestigatorId,
    OpenWindow, PendingTrigger, Phase, Status, WindowKind,
};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};

/// Queue a reaction window of the given `kind` if any in-play card
/// has a matching `Trigger::OnEvent` ability. No-op when the registry
/// isn't installed or no card matches.
///
/// Emits [`Event::WindowOpened`] before pushing onto
/// [`GameState::open_windows`] so reaction-window observability is
/// symmetric with the Fast-window path ([`open_fast_window`]).
/// If no triggers are pending the function returns early without
/// emitting anything — the window never opens.
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
pub(super) fn queue_reaction_window(
    state: &mut GameState,
    events: &mut Vec<Event>,
    kind: WindowKind,
) {
    let pending_triggers = scan_pending_triggers(state, kind);
    if pending_triggers.is_empty() {
        return;
    }
    events.push(Event::WindowOpened { kind });
    // Reaction windows admit any investigator's Fast actions
    // (Rules Reference: Fast may be played at any player window).
    // Multi-window nesting is now structural — push twice is valid.
    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });
}

/// Scan every investigator's `cards_in_play` for
/// `Trigger::OnEvent` abilities matching `kind`, building a pending-
/// trigger list in active-investigator-first / turn-order resolution
/// order.
///
/// Returns an empty vec when the registry isn't installed (tests that
/// don't touch card data) or no cards match.
fn scan_pending_triggers(state: &GameState, kind: WindowKind) -> Vec<PendingTrigger> {
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

    let mut pending: Vec<PendingTrigger> = Vec::new();
    for id in order {
        let Some(inv) = state.investigators.get(&id) else {
            continue;
        };
        for card in &inv.cards_in_play {
            let Some(abilities) = (reg.abilities_for)(&card.code) else {
                continue;
            };
            for (idx, ability) in abilities.iter().enumerate() {
                let Trigger::OnEvent { pattern, timing } = ability.trigger else {
                    continue;
                };
                if !trigger_matches(kind, pattern, timing, id) {
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
                // Phase-3 scope: every queued trigger is optional.
                // The DSL has no forced primitive yet (#52 doc).
                pending.push(PendingTrigger {
                    controller: id,
                    instance_id: card.instance_id,
                    ability_index,
                    forced: false,
                });
            }
        }
    }
    pending
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
    pattern: EventPattern,
    timing: EventTiming,
    controller: InvestigatorId,
) -> bool {
    if timing != EventTiming::After {
        return false;
    }
    match (kind, pattern) {
        (
            WindowKind::AfterEnemyDefeated { by, .. },
            EventPattern::EnemyDefeated { by_controller },
        ) => {
            if by_controller {
                by == Some(controller)
            } else {
                true
            }
        }
        // BetweenPhases, MythosAfterDraws, UpkeepBegins,
        // BeforeInvestigatorAttacked, AfterAllInvestigatorsAttacked,
        // InvestigationBegins, and InvestigatorTurnBegins windows open
        // for timing reasons; no Trigger::OnEvent pattern
        // matches them — those windows gate Fast actions, not
        // after-event reactions. AfterEnemyDefeated windows only match
        // EnemyDefeated patterns (handled above); encounter-reveal
        // patterns return false.
        //
        // EnemySpawned: no WindowKind opens specifically for "enemy
        // spawned" in Phase 4. A future PR (likely Phase-7+) that wants
        // to react to spawns will add the corresponding WindowKind
        // variant and update this arm.
        (
            WindowKind::BetweenPhases { .. }
            | WindowKind::AfterEnemyDefeated { .. }
            | WindowKind::MythosAfterDraws
            | WindowKind::UpkeepBegins
            | WindowKind::BeforeInvestigatorAttacked
            | WindowKind::AfterAllInvestigatorsAttacked
            | WindowKind::InvestigationBegins
            | WindowKind::InvestigatorTurnBegins,
            EventPattern::EnemyDefeated { .. }
            | EventPattern::CardRevealed { .. }
            | EventPattern::EnemySpawned,
        ) => false,
    }
}

/// Return [`AwaitingInput`] for the already-open reaction window at
/// the top of [`GameState::open_windows`]. Called by [`drive_skill_test`]
/// at a step boundary when an earlier step queued a window via
/// [`queue_reaction_window`].
///
/// [`Event::WindowOpened`] is emitted by [`queue_reaction_window`]
/// (not here) so the event appears at queue time and is symmetric with
/// the [`open_fast_window`] path.
pub(super) fn open_queued_reaction_window(
    state: &GameState,
    _events: &mut Vec<Event>,
) -> EngineOutcome {
    let window = state
        .top_reaction_window()
        .expect("open_queued_reaction_window: caller checked is_some");
    EngineOutcome::AwaitingInput {
        request: InputRequest {
            prompt: format!(
                "Reaction window {:?}: {} trigger(s) pending. \
                 Submit InputResponse::PickIndex to fire one, or \
                 InputResponse::Skip to close.",
                window.kind,
                window.pending_triggers.len(),
            ),
        },
        // No multi-window state to disambiguate — routing keys off
        // the top of `state.open_windows`. Conventional 0 like the
        // commit-window's resume token.
        resume_token: ResumeToken(0),
    }
}

/// Resume an open reaction window with the player's response.
///
/// - [`InputResponse::PickIndex(i)`]: fires the i-th pending trigger
///   via the evaluator. After firing, removes the entry. If pending
///   triggers remain, re-emits [`AwaitingInput`]; else closes the
///   window.
/// - [`InputResponse::Skip`]: closes the window provided no forced
///   triggers remain. Rejects when forced triggers are still pending.
/// - Other variants reject; the window stays open.
///
/// Closing the window emits [`Event::WindowClosed`] with the same
/// kind, pops the top entry from [`GameState::open_windows`], and
/// returns [`Done`].
pub(super) fn resume_reaction_window(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    match response {
        InputResponse::PickIndex(i) => fire_pending_trigger(state, events, *i),
        InputResponse::Skip => {
            // Resolve the active reaction-window index up-front so the
            // close path operates on the same window the driver had
            // been driving (not the absolute top of `open_windows`,
            // which may be an empty-pending_triggers window sitting
            // above it).
            let idx = state
                .top_reaction_window_index()
                .expect("resume_reaction_window: caller checked is_some");
            close_reaction_window_at(state, events, idx)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: reaction window expects InputResponse::PickIndex \
                 or InputResponse::Skip, got {other:?}",
            )
            .into(),
        },
    }
}

/// Fire the pending trigger at index `i` in the open reaction window.
/// Rejects out-of-bounds; the window stays open so the client can
/// retry with a corrected index.
fn fire_pending_trigger(state: &mut GameState, events: &mut Vec<Event>, i: u32) -> EngineOutcome {
    // Capture the index of the reaction window we're driving up-front
    // so the close path removes the same entry (not the absolute top of
    // the stack, which may be a different, empty-pending_triggers
    // window once non-reaction windows can sit above one).
    let window_idx = state
        .top_reaction_window_index()
        .expect("fire_pending_trigger: caller checked is_some");
    // Snapshot to avoid borrowing state across the apply_effect call.
    let (trigger, pending_idx) = {
        let window = &state.open_windows[window_idx];
        let idx = match usize::try_from(i) {
            Ok(idx) if idx < window.pending_triggers.len() => idx,
            _ => {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: reaction-window PickIndex({i}) out of bounds \
                         (pending size {})",
                        window.pending_triggers.len(),
                    )
                    .into(),
                };
            }
        };
        (window.pending_triggers[idx], idx)
    };

    // Look up the ability fresh from the registry. The card may have
    // changed state between scan and fire (exhausted, used, …) but
    // its ability list is static, so registry lookup is sufficient.
    let Some(reg) = card_registry::current() else {
        unreachable!(
            "fire_pending_trigger: registry was installed at scan time but is now \
             missing; the OnceLock contract guarantees once-set-stays-set"
        );
    };
    let inv = state
        .investigators
        .get(&trigger.controller)
        .unwrap_or_else(|| {
            unreachable!(
                "fire_pending_trigger: controller {ctl:?} vanished while reaction window \
                 was open; this is a state-corruption invariant violation",
                ctl = trigger.controller,
            )
        });
    let card = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == trigger.instance_id)
        .unwrap_or_else(|| {
            unreachable!(
                "fire_pending_trigger: instance {inst:?} vanished from controller {ctl:?}'s \
                 cards_in_play while reaction window was open; state-corruption invariant \
                 violation",
                inst = trigger.instance_id,
                ctl = trigger.controller,
            )
        });
    let code = card.code.clone();
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

    // Thread the source instance into the EvalContext so effects that
    // push `PendingSkillModifier`s (or any other source-attributed
    // state) can attribute them to the firing card. Phase-3 reaction
    // effects (`discover_clue`, `gain_resources`) don't read this,
    // but the first source-attributing reaction effect will, and the
    // information is already on the trigger record.
    let ctx = EvalContext::for_controller_with_source(trigger.controller, trigger.instance_id);
    let usage_limit = ability.usage_limit;
    let result = apply_effect(state, events, &ability.effect, ctx);
    if let EngineOutcome::Rejected { reason } = result {
        // Card-impl bugs surface loudly — same policy as
        // `fire_on_skill_test_resolution`.
        unreachable!("OnEvent reaction: effect for card {code:?} rejected unexpectedly: {reason}");
    }

    if usage_limit.is_some() {
        bump_usage_counter(state, &trigger);
    }

    // Drop the fired entry now that resolution succeeded. The window
    // we drove sits at `window_idx` — apply_effect does not push or
    // pop `open_windows` entries, so the index remains valid.
    let window = &mut state.open_windows[window_idx];
    window.pending_triggers.remove(pending_idx);

    // If more triggers remain pending, re-emit AwaitingInput so the
    // player can pick the next one. Otherwise the window closes
    // automatically.
    if window.pending_triggers.is_empty() {
        return close_reaction_window_at(state, events, window_idx);
    }
    let kind = window.kind;
    let pending_len = window.pending_triggers.len();
    EngineOutcome::AwaitingInput {
        request: InputRequest {
            prompt: format!(
                "Reaction window {kind:?}: {pending_len} trigger(s) pending. \
                 Submit InputResponse::PickIndex to fire one, or \
                 InputResponse::Skip to close.",
            ),
        },
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
fn bump_usage_counter(state: &mut GameState, trigger: &PendingTrigger) {
    let current_round = state.round;
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
        .find(|c| c.instance_id == trigger.instance_id)
        .unwrap_or_else(|| {
            unreachable!(
                "bump_usage_counter: instance {inst:?} vanished from controller {ctl:?}'s \
                 cards_in_play while reaction window was open; state-corruption invariant \
                 violation",
                inst = trigger.instance_id,
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
/// top of the stack — once `BetweenPhases` (or any other
/// non-reaction) window can sit above an active reaction window
/// (#69/#70/#71), a naive `open_windows.pop()` would remove the wrong
/// entry. Callers compute the index via
/// [`GameState::top_reaction_window_index`].
pub(super) fn close_reaction_window_at(
    state: &mut GameState,
    events: &mut Vec<Event>,
    idx: usize,
) -> EngineOutcome {
    // Borrow the window at `idx` to check for forced triggers remaining
    // before removing — Rejected must leave state untouched.
    {
        let window = state
            .open_windows
            .get(idx)
            .expect("close_reaction_window_at: caller-supplied index must be in bounds");
        if let Some(forced) = window.pending_triggers.iter().find(|t| t.forced) {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput::Skip: cannot close reaction window while forced trigger \
                     (controller {ctl:?}, instance {inst:?}, ability {ab}) remains pending; \
                     fire it first",
                    ctl = forced.controller,
                    inst = forced.instance_id,
                    ab = forced.ability_index,
                )
                .into(),
            };
        }
    }
    let window = state.open_windows.remove(idx);
    let kind = window.kind;
    events.push(Event::WindowClosed { kind });

    // Run any kind-specific continuation (e.g. MythosAfterDraws →
    // mythos_phase_end). For reaction windows that have no continuation
    // (AfterEnemyDefeated, BetweenPhases) this is a no-op.
    run_window_continuation(state, events, kind);

    // If a skill test was mid-resolution when this window opened,
    // hand control back to its driver to run the remaining steps.
    // `AwaitingCommit` means the test is parked at the commit
    // window (no driver state to resume); this happens when a future
    // non-skill-test action queues a window — `Done` is the right
    // terminal outcome.
    if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
        if !matches!(in_flight.continuation, FinishContinuation::AwaitingCommit) {
            return super::skill_test::drive_skill_test(state, events);
        }
    }

    EngineOutcome::Done
}

/// Kind-aware continuation called when a window closes (whether
/// inline via [`open_fast_window`]'s auto-skip path or via the
/// [`close_reaction_window_at`] pop path). For
/// [`WindowKind::MythosAfterDraws`], runs
/// [`mythos_phase_end`](super::mythos_phase_end); for
/// [`WindowKind::UpkeepBegins`], runs [`upkeep_resume`](super::upkeep_resume). For
/// [`WindowKind::BeforeInvestigatorAttacked`], resolves the cursor
/// investigator's engaged-enemy attacks via
/// [`resolve_attacks_for_investigator`](super::combat::resolve_attacks_for_investigator), advances
/// [`GameState::enemy_attack_pending`] to the next Active investigator
/// via [`cursor::next_active_investigator_after`](super::cursor::next_active_investigator_after),
/// and opens the next window
/// ([`WindowKind::BeforeInvestigatorAttacked`] again if the cursor
/// advanced to `Some`, otherwise [`WindowKind::AfterAllInvestigatorsAttacked`]).
/// For [`WindowKind::AfterAllInvestigatorsAttacked`], runs
/// [`enemy_phase_end`](super::enemy_phase_end). For [`WindowKind::InvestigationBegins`], starts
/// the first turn via [`begin_investigator_turn`](super::begin_investigator_turn) for the first Active
/// investigator (or parks if none). `AfterEnemyDefeated`, `BetweenPhases`,
/// and [`WindowKind::InvestigatorTurnBegins`] windows have no
/// continuation — for them this is a no-op preserving the existing
/// [`close_reaction_window_at`] behavior.
pub(super) fn run_window_continuation(
    state: &mut GameState,
    events: &mut Vec<Event>,
    kind: WindowKind,
) {
    match kind {
        WindowKind::MythosAfterDraws => {
            // Phase-transitioning continuation: cannot run while a skill
            // test is in flight (would strand the test in the wrong
            // phase). Phase 4 has no Mythos-phase skill-test sources, so
            // this branch is structurally unreachable today. A future PR
            // adding a Mythos-phase Revelation that initiates a skill
            // test must redesign the close-window + phase-transition
            // ordering before this assertion fires.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "MythosAfterDraws window closed while a skill test is in flight \
                     (continuation={:?}). Phase transition would strand the skill test \
                     in the wrong phase. Phase 4 has no Mythos-phase skill test sources; \
                     if a future PR adds one (e.g. a treachery whose Revelation initiates \
                     a skill test), the window-close + phase-transition ordering needs \
                     redesign before this assertion can be relaxed.",
                    in_flight.continuation,
                );
            }
            super::mythos_phase_end(state, events);
        }
        WindowKind::UpkeepBegins => {
            // Phase-transitioning continuation (4.2–4.6 then Upkeep→Mythos):
            // cannot run while a skill test is in flight. Phase 4 has no
            // Upkeep-phase skill-test source, so structurally unreachable.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "UpkeepBegins window closed while a skill test is in flight \
                     (continuation={:?}). Phase 4 has no Upkeep-phase skill-test \
                     sources; a future PR adding one needs the window-close + \
                     phase-transition ordering redesigned before this fires.",
                    in_flight.continuation,
                );
            }
            super::upkeep_resume(state, events);
        }
        WindowKind::BeforeInvestigatorAttacked => {
            // Phase-transitioning continuation (advances to the next
            // window and ultimately to Upkeep) — cannot run while a
            // skill test is in flight (would strand it). Phase 4 has
            // no Enemy-phase skill-test source, so this branch is
            // structurally unreachable today. A future PR adding one
            // (e.g. a treachery-style "make an Agility test or take
            // damage" attack ability) must redesign the window-close
            // + phase-transition ordering before this assertion fires.
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "BeforeInvestigatorAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase \
                     transition would strand the skill test in the \
                     wrong phase. Phase 4 has no Enemy-phase skill test \
                     sources; if a future PR adds one, the window-close \
                     + phase-transition ordering needs redesign before \
                     this assertion can be relaxed.",
                    in_flight.continuation,
                );
            }

            // Cursor expect-Some: BeforeInvestigatorAttacked is only
            // ever opened after enemy_attack_pending is set to Some(_)
            // in enemy_phase or in the advance below. A None cursor
            // here is a state-corruption invariant violation, not a
            // normal rejection path.
            let investigator = state.enemy_attack_pending.unwrap_or_else(|| {
                unreachable!(
                    "BeforeInvestigatorAttacked closed with \
                     enemy_attack_pending == None; this is a \
                     state-corruption invariant violation"
                )
            });

            super::combat::resolve_attacks_for_investigator(state, events, investigator);

            // Advance the cursor: next Active investigator AFTER
            // `investigator` in turn_order. The shared helper uses
            // turn_order (not the filtered-Active list) as the index
            // basis, so `investigator` itself can have been defeated
            // mid-loop and we still find the right successor.
            state.enemy_attack_pending =
                super::cursor::next_active_investigator_after(state, investigator);

            if state.enemy_attack_pending.is_some() {
                open_fast_window(state, events, WindowKind::BeforeInvestigatorAttacked);
            } else {
                open_fast_window(state, events, WindowKind::AfterAllInvestigatorsAttacked);
            }
        }
        WindowKind::AfterAllInvestigatorsAttacked => {
            if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
                unreachable!(
                    "AfterAllInvestigatorsAttacked window closed while a \
                     skill test is in flight (continuation={:?}). Phase \
                     4 has no Enemy-phase skill-test sources; a future \
                     PR adding one needs the window-close + \
                     phase-transition ordering redesigned before this \
                     fires.",
                    in_flight.continuation,
                );
            }
            super::enemy_phase_end(state, events);
        }
        WindowKind::InvestigationBegins => {
            // Post-2.1 window closed; start the first turn (step 2.2).
            // No skill-test-in-flight guard: this runs at phase start
            // (no test can be in flight) and does not transition phase.
            if let Some(id) = super::cursor::first_active_investigator(state) {
                super::begin_investigator_turn(state, events, id);
            }
            // None branch: no active investigator can take a turn. Per
            // Rules Reference p.10 step 6 the scenario ends — that
            // resolution now fires at the defeat site:
            // `check_all_defeated` latches `Resolution::Lost` (and emits
            // `AllInvestigatorsDefeated`), which the `apply` hook turns
            // into `ScenarioResolved` + `apply_resolution`. So by the
            // time the cascade would re-enter Investigation with no
            // active investigator, the loss has already resolved; this
            // park branch stays as the cascade-breaker (auto-advancing
            // would loop the round forever — every other phase auto-skips
            // with no active investigators, so Investigation is the
            // cascade's only natural pause point).
        }
        // InvestigatorTurnBegins: 2.2.1 — the active investigator now
        // takes actions (Investigate / Move / Fight / Evade / PlayCard /
        // Draw / ActivateAbility) as player-driven inputs, then ends the
        // turn via EndTurn (2.2.2). No continuation work — the engine
        // waits. The per-action "return to the previous player window"
        // re-open (Rules Reference p.24 2.2.1) is deferred to #146.
        WindowKind::AfterEnemyDefeated { .. }
        | WindowKind::BetweenPhases { .. }
        | WindowKind::InvestigatorTurnBegins => {}
    }
}

/// Open a printed Fast-play window of the given kind. Always emits
/// [`Event::WindowOpened`] for observability. Then either:
///
/// - Pushes the [`OpenWindow`] onto [`GameState::open_windows`] if any
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
pub(super) fn open_fast_window(state: &mut GameState, events: &mut Vec<Event>, kind: WindowKind) {
    events.push(Event::WindowOpened { kind });

    // Push first so any_fast_play_eligible's check_play_card call sees
    // this window in state.open_windows when evaluating permissive_window.
    let pending_triggers = scan_pending_triggers(state, kind);
    state.open_windows.push(OpenWindow {
        kind,
        pending_triggers,
        fast_actors: FastActorScope::Any,
    });

    let has_pending = !state
        .open_windows
        .last()
        .expect("just pushed; cannot be empty")
        .pending_triggers
        .is_empty();
    let has_fast_eligible = any_fast_play_eligible(state);

    if !has_pending && !has_fast_eligible {
        // Auto-skip: nothing to do. Pop the window we just pushed,
        // emit WindowClosed, and run the continuation inline, so the
        // net effect on state.open_windows is the same as before the fix.
        let _ = state.open_windows.pop();
        events.push(Event::WindowClosed { kind });
        run_window_continuation(state, events, kind);
    }
    // Otherwise the window stays on the stack. The guard at the top of
    // apply() and resume_reaction_window / resolve_input handle the
    // wait + close path.
}

/// Pure-validation peer to [`play_card`]. Returns `Ok` if the named
/// card is currently playable by `investigator`, `Err(reason)` if
/// not. The check is the existing `play_card` validation block lifted
/// verbatim — no behavior change at `play_card`'s call site.
///
/// Used by [`play_card`] (which then runs the mutation block on the
/// `Ok` payload) and by `any_fast_play_eligible` (which only
/// inspects `Ok` vs `Err`).
pub(super) fn check_play_card(
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
    // Timing gate — see play_card doc-comment "# Timing gate" section.
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let owner_is_active = state.active_investigator == Some(investigator);
    let permissive_window = state
        .open_windows
        .last()
        .is_some_and(|w| w.fast_actors.permits(investigator));
    // Non-asset/non-event card types are filtered out by
    // `resolve_play_target` above, so `card_type` here is always one of
    // `Asset` or `Event`. The non-Fast arm collapses both into the
    // strict gate; the Fast arms split because Rules Reference p. 11
    // gives events and assets different scopes (any vs owner-only).
    let allowed = if is_fast {
        match card_type {
            CardType::Event => active_during_investigation || permissive_window,
            CardType::Asset => {
                active_during_investigation || (owner_is_active && permissive_window)
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

/// Pure-validation peer to [`activate_ability`]. Mirrors
/// [`check_play_card`]: validation block lifted verbatim, no behavior
/// change at the call site.
///
/// Returns `Ok(ActivateCheckResult)` if the ability is currently
/// activatable, `Err(reason)` otherwise. Does not mutate state.
pub(super) fn check_activate_ability(
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
        .open_windows
        .last()
        .is_some_and(|w| w.fast_actors.permits(investigator));
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
        if let Err(reason) = super::abilities::check_cost_payable(cost, inv, source_exhausted) {
            return Err(reason.into());
        }
    }

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
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn check_play_card_returns_err_for_unknown_hand_index() {
        let state = TestGame::default()
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
        let state = TestGame::default().build();
        let err = check_play_card(&state, InvestigatorId(99), 0)
            .expect_err("missing investigator should reject");
        assert!(
            err.contains("not in state"),
            "error should say not in state, got: {err}"
        );
    }
}

#[cfg(test)]
mod check_activate_ability_tests {
    use super::*;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn check_activate_ability_returns_err_for_missing_instance() {
        let state = TestGame::default()
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
        let state = TestGame::default().build();
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
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn returns_false_when_no_investigators() {
        let state = TestGame::default().build();
        assert!(!any_fast_play_eligible(&state));
    }

    #[test]
    fn returns_false_when_hands_and_in_play_empty() {
        let state = TestGame::default()
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
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn open_fast_window_with_no_eligibility_emits_open_then_close_inline() {
        // No reactions, no Fast-eligible cards → auto-skip: window
        // opens and closes without ever landing on state.open_windows.
        let mut state = TestGame::default()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        open_fast_window(&mut state, &mut events, WindowKind::MythosAfterDraws);

        assert!(
            state.open_windows.is_empty(),
            "auto-skip must not leave the window on the stack"
        );
        assert!(
            matches!(
                events.first(),
                Some(Event::WindowOpened {
                    kind: WindowKind::MythosAfterDraws
                })
            ),
            "first event must be WindowOpened; got {:?}",
            events.first()
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::WindowClosed {
                    kind: WindowKind::MythosAfterDraws
                }
            )),
            "must emit WindowClosed for MythosAfterDraws; events = {events:?}"
        );
    }

    #[test]
    fn run_window_continuation_for_no_continuation_kind_does_nothing() {
        // AfterEnemyDefeated has no continuation. Calling it must be a
        // no-op (no events, no state change).
        let mut state = TestGame::default().build();
        let mut events = Vec::new();
        run_window_continuation(
            &mut state,
            &mut events,
            WindowKind::AfterEnemyDefeated {
                enemy: EnemyId(1),
                by: None,
            },
        );
        assert!(
            events.is_empty(),
            "AfterEnemyDefeated continuation must be a no-op; events = {events:?}"
        );
    }
}
