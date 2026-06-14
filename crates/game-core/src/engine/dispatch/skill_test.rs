//! Skill-test resolution handlers.
//!
//! Contains the full skill-test lifecycle: starting a test
//! ([`start_skill_test`]), the commit-stage entry ([`finish_skill_test`]),
//! the resolution driver ([`drive_skill_test`]), and all supporting
//! helpers.

use std::collections::BTreeSet;

use crate::card_registry;
use crate::dsl::{discover_clue, LocationTarget, SkillTestKind, Trigger};
use crate::event::{Event, FailureReason};
use crate::state::{
    resolve_token, CardCode, ChaosToken, FinishContinuation, GameState, InFlightSkillTest,
    InvestigatorId, SkillKind, SkillTestFollowUp, Status, TokenResolution, Zone,
};

use super::super::evaluator::{
    apply_effect, constant_skill_modifier, pending_skill_modifier, EvalContext,
};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

// Nine args: the skill-test parameters are genuinely independent axes
// (skill, kind, difficulty, success/fail follow-ups, source). A params
// struct would add indirection without grouping anything cohesive.
#[allow(clippy::too_many_arguments)]
pub(in crate::engine) fn start_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty: i8,
    follow_up: SkillTestFollowUp,
    on_success: Option<card_dsl::dsl::Effect>,
    on_fail: Option<card_dsl::dsl::Effect>,
    source: Option<crate::state::CardInstanceId>,
) -> EngineOutcome {
    // Validate-first: investigator must exist and be Active; chaos
    // bag must be non-empty so we can draw; difficulty must be non-
    // negative (FFG difficulties are always ≥ 0). Defeated
    // investigators can't take skill tests — they're out of play.
    // A second test cannot overlap an in-flight one.
    let Some(inv) = cx.state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("skill test: investigator {investigator:?} not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "skill test: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if cx.state.chaos_bag.tokens.is_empty() {
        return EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        };
    }
    if difficulty < 0 {
        return EngineOutcome::Rejected {
            reason: format!("skill test: difficulty {difficulty} must be >= 0").into(),
        };
    }
    if cx.state.in_flight_skill_test.is_some() {
        return EngineOutcome::Rejected {
            reason: "skill test: another skill test is already in flight; only one test \
                     may pause at a commit window at a time"
                .into(),
        };
    }

    // Mutate-second: stash the in-flight record and announce the test.
    // Snapshot the investigator's location for
    // `LocationTarget::TestedLocation` resolution during
    // `Trigger::OnSkillTestResolution` firing. `inv`'s immutable
    // borrow from the validation block above is still live; reading
    // `current_location` here doesn't extend it past this line.
    let tested_location = inv.current_location;
    cx.state.in_flight_skill_test = Some(InFlightSkillTest {
        investigator,
        skill,
        kind,
        difficulty,
        committed_by_active: Vec::new(),
        tested_location,
        follow_up,
        on_fail,
        on_success,
        source,
        continuation: FinishContinuation::AwaitingCommit,
    });
    cx.events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    EngineOutcome::AwaitingInput {
        request: InputRequest {
            prompt: format!(
                "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
                 (difficulty {difficulty}). Empty indices commits no cards.",
            ),
        },
        // Routing keys off `state.in_flight_skill_test`, not the
        // token, so any opaque value is fine here. ResumeToken(0) is
        // the conventional "no extra context needed" choice for the
        // first AwaitingInput site.
        resume_token: ResumeToken(0),
    }
}

/// Commit-stage entry to the skill-test resolution driver. Handles
/// the response to the
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput) the engine
/// emitted at the commit window: validate the supplied indices, sum
/// the committed cards' icon contribution (matching skill + wild),
/// draw a chaos token, emit the success/failure events, apply the
/// action-specific [`SkillTestFollowUp`] on success, then hand off to
/// [`drive_skill_test`] for the remaining steps.
///
/// The split between this entry and [`drive_skill_test`] exists so
/// that a reaction window opening *inside*
/// [`apply_skill_test_follow_up`] (the canonical case:
/// `damage_enemy` emitting [`EnemyDefeated`](crate::Event::EnemyDefeated)
/// queues an [`AfterEnemyDefeated`](crate::state::WindowKind::AfterEnemyDefeated)
/// window) suspends correctly: this entry advances the continuation
/// to [`FinishContinuation::PostFollowUp`] before delegating, so a
/// resume from `close_reaction_window_at` re-enters the driver and picks
/// up at the `OnSkillTestResolution` step.
///
/// On invalid input (no in-flight test, malformed indices, or
/// continuation already advanced) returns [`EngineOutcome::Rejected`]
/// with no state change and no events pushed — the engine stays
/// paused so the caller can submit a fixed-up response.
///
/// [`close_reaction_window_at`]: super::reaction_windows::close_reaction_window_at
pub(super) fn finish_skill_test(cx: &mut Cx, indices: &[u32]) -> EngineOutcome {
    // Snapshot the in-flight record (Copy-able primitives only) so
    // later mutation paths can re-borrow state freely.
    let Some(in_flight) = cx.state.in_flight_skill_test.as_ref() else {
        return EngineOutcome::Rejected {
            reason: "ResolveInput::CommitCards: no in-flight skill test to resume".into(),
        };
    };
    if !matches!(in_flight.continuation, FinishContinuation::AwaitingCommit) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput::CommitCards: commit window already closed (continuation {:?}); \
                 the engine is mid-resolution, not at the commit step",
                in_flight.continuation,
            )
            .into(),
        };
    }
    let investigator = in_flight.investigator;
    let skill = in_flight.skill;
    let kind = in_flight.kind;
    let difficulty = in_flight.difficulty;
    let follow_up = in_flight.follow_up;
    let on_fail = in_flight.on_fail.clone();
    let on_success = in_flight.on_success.clone();
    let source = in_flight.source;

    // Validate the commit indices against the resolving
    // investigator's hand. On Err, state is untouched and the engine
    // stays paused so the client can retry.
    let indices_u8 = match validate_commit_indices(cx.state, investigator, indices) {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };

    let skill_value = sum_skill_value(cx.state, investigator, skill, kind, &indices_u8);

    // Persist the committed indices into the in-flight record for
    // replay clarity. Safe to expect: we read `in_flight_skill_test`
    // immediately above and nothing has cleared it since.
    cx.state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .committed_by_active
        .clone_from(&indices_u8);

    let (succeeded, failed_by) =
        resolve_chaos_token_and_emit(cx, investigator, skill, difficulty, skill_value);

    // Build the eval context for the success/failure card effects,
    // threading the firing instance (`source`) so `Effect::DiscardSelf`
    // can find itself across the suspend/resume boundary.
    let card_ctx = |investigator: InvestigatorId| match source {
        Some(src) => EvalContext::for_controller_with_source(investigator, src),
        None => EvalContext::for_controller(investigator),
    };

    // Pre-advance the continuation to PostFollowUp BEFORE running the
    // follow-up, so a follow-up that suspends on a clue-discovery interrupt
    // (Cover Up 01007) resumes at PostFollowUp rather than re-running the
    // follow-up. `on_success` never co-occurs with a suspending follow-up
    // in scope (Investigate sets on_success=None; SkillTest-effect tests
    // set follow_up=None), so running on_success after the follow-up is
    // safe. (C5a #236.)
    cx.state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .continuation = FinishContinuation::PostFollowUp { succeeded };

    if succeeded {
        let outcome = apply_skill_test_follow_up(cx, investigator, follow_up);
        if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
            // The follow-up suspended (clue-discovery interrupt). The
            // continuation is already PostFollowUp; resume re-enters the
            // driver there. Don't run on_success now — it doesn't co-occur
            // with a suspending follow-up in scope.
            return outcome;
        }
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "skill-test follow-up must resolve to Done or AwaitingInput: {outcome:?}"
        );
        if let Some(effect) = &on_success {
            // Success-side card effect (Frozen in Fear 01164 discards
            // itself on a successful end-of-turn willpower test). In-scope
            // effects run to completion; a future suspending on_success is
            // #212 reentrancy work.
            let outcome = apply_effect(cx, effect, card_ctx(investigator));
            debug_assert!(
                matches!(outcome, EngineOutcome::Done),
                "skill-test on_success must resolve to Done in scope: {outcome:?}"
            );
        }
    } else if let Some(effect) = &on_fail {
        // Margin-keyed failure branch of a treachery-Revelation test
        // (`Effect::SkillTest`). The failure margin is threaded so
        // `Effect::ForEachPointFailed` can scale. In-scope on_fail
        // effects (DealDamage / DealHorror / Native) run to completion;
        // a future suspending on_fail is #212 reentrancy work.
        let mut ctx = card_ctx(investigator);
        ctx.failed_by = Some(failed_by);
        let outcome = apply_effect(cx, effect, ctx);
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "revelation on_fail must resolve to Done in C4b scope: {outcome:?}"
        );
    }

    drive_skill_test(cx)
}

/// Walk the skill-test resolution sequence from the current
/// [`FinishContinuation`] onward, suspending if a reaction window
/// queues mid-step.
///
/// Each loop iteration starts by checking for a queued reaction
/// window: if one is pending, the driver emits
/// [`Event::WindowOpened`](crate::Event::WindowOpened) and returns
/// [`AwaitingInput`](crate::EngineOutcome::AwaitingInput). The window's
/// close path ([`close_reaction_window_at`]) re-enters this driver on
/// resume.
///
/// Step → next-continuation mapping (current Phase-3 set; #64 will
/// add the post-`SkillTestEnded` window between
/// [`PostOnResolution`](FinishContinuation::PostOnResolution) and
/// teardown):
///
/// - [`PostFollowUp`](FinishContinuation::PostFollowUp) → fire
///   `OnSkillTestResolution` triggers; advance to
///   [`PostOnResolution`](FinishContinuation::PostOnResolution).
/// - [`PostOnResolution`](FinishContinuation::PostOnResolution) →
///   discard committed cards, emit
///   [`SkillTestEnded`](crate::Event::SkillTestEnded), drain pending
///   modifiers, clear in-flight, return `Done`.
///
/// [`close_reaction_window_at`]: super::reaction_windows::close_reaction_window_at
pub(super) fn drive_skill_test(cx: &mut Cx) -> EngineOutcome {
    loop {
        if cx.state.top_reaction_window().is_some() {
            return super::reaction_windows::open_queued_reaction_window(cx);
        }

        let (continuation, investigator, indices_u8) = {
            let in_flight = cx.state.in_flight_skill_test.as_ref().unwrap_or_else(|| {
                unreachable!(
                    "drive_skill_test: in_flight_skill_test must exist while driver is active; \
                     state-corruption invariant violation"
                )
            });
            (
                in_flight.continuation,
                in_flight.investigator,
                in_flight.committed_by_active.clone(),
            )
        };

        match continuation {
            FinishContinuation::AwaitingCommit => {
                unreachable!(
                    "drive_skill_test: entered with AwaitingCommit; the commit-stage entry \
                     (finish_skill_test) advances past this before delegating"
                );
            }
            FinishContinuation::PostFollowUp { succeeded } => {
                fire_on_skill_test_resolution(cx, investigator, &indices_u8, succeeded);
                cx.state
                    .in_flight_skill_test
                    .as_mut()
                    .expect("in_flight_skill_test must persist across driver steps")
                    .continuation = FinishContinuation::PostRetaliate { succeeded };
            }
            FinishContinuation::PostRetaliate { succeeded } => {
                fire_retaliate_if_any(cx, investigator, succeeded);
                cx.state
                    .in_flight_skill_test
                    .as_mut()
                    .expect("in_flight_skill_test must persist across driver steps")
                    .continuation = FinishContinuation::PostOnResolution { succeeded };
            }
            FinishContinuation::PostOnResolution { succeeded } => {
                fire_after_location_investigated(cx, investigator, succeeded);
                discard_committed_cards(cx, investigator, &indices_u8);
                cx.events.push(Event::SkillTestEnded { investigator });
                // ModifierScope::ThisSkillTest contributions expire when
                // the test ends. Drain pending entries for *this*
                // investigator only — entries queued for other
                // investigators' future tests stay.
                cx.state
                    .pending_skill_modifiers
                    .retain(|m| m.investigator != investigator);
                // A treachery whose Revelation suspended into this test
                // discards once the test fully resolves (the discard
                // step `resolve_encounter_card` skipped on suspend).
                // Eventless push, matching the normal treachery-discard
                // path in `resolve_encounter_card`.
                if let Some(code) = cx.state.pending_revelation_discard.take() {
                    cx.state.encounter_discard.push(code);
                }
                cx.state.in_flight_skill_test = None;
                return EngineOutcome::Done;
            }
        }
    }
}

/// Validate that every entry in `indices` is a unique in-bounds hand
/// index for `investigator`, and return them downcast to `u8` (the
/// width hand indices use elsewhere in state).
fn validate_commit_indices(
    state: &GameState,
    investigator: InvestigatorId,
    indices: &[u32],
) -> Result<Vec<u8>, EngineOutcome> {
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "validate_commit_indices: investigator {investigator:?} disappeared while test \
             was in flight; this is a state-corruption invariant violation"
        )
    });
    // Arkham's upkeep hand-size limit caps hands well below 256 cards
    // in practice (#111 tracks the engine-side enforcement of the
    // discard-to-max-hand-size step), so the `u8::try_from` below
    // succeeds for every index that passed the bounds check. No
    // defensive overflow-rejection branch needed.
    let hand_len = inv.hand.len();
    let mut indices_u8: Vec<u8> = Vec::with_capacity(indices.len());
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for &i in indices {
        if !seen.insert(i) {
            return Err(EngineOutcome::Rejected {
                reason: format!("CommitCards: duplicate hand index {i}").into(),
            });
        }
        if (i as usize) >= hand_len {
            return Err(EngineOutcome::Rejected {
                reason: format!("CommitCards: hand index {i} out of bounds (hand size {hand_len})")
                    .into(),
            });
        }
        indices_u8.push(
            u8::try_from(i)
                .expect("bounds check above guarantees i < hand_len <= u8::MAX (see #111)"),
        );
    }
    Ok(indices_u8)
}

/// Sum the four skill-value contributions: investigator's printed
/// stat, constant modifiers from cards in play, queued
/// [`ModifierScope::ThisSkillTest`] pushes, and the committed cards'
/// matching + wild icons.
///
/// Cards / scopes not addressed by an installed registry contribute
/// 0 — the same silent-skip policy `constant_skill_modifier` uses.
///
/// [`ModifierScope::ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
fn sum_skill_value(
    state: &GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    committed_indices: &[u8],
) -> i8 {
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "sum_skill_value: investigator {investigator:?} disappeared while test was in \
             flight; this is a state-corruption invariant violation"
        )
    });
    let base = inv.skills.value(skill);
    let icon_mod = sum_committed_icons(&inv.hand, committed_indices, skill);
    let constant_mod = card_registry::current().map_or(0, |reg| {
        constant_skill_modifier(state, reg, investigator, skill, kind)
    });
    let pending_mod = pending_skill_modifier(state, investigator, skill);
    base.saturating_add(constant_mod)
        .saturating_add(pending_mod)
        .saturating_add(icon_mod)
}

/// Sum the skill-icon contribution from the cards at `indices` in
/// `hand`: each card adds its matching-skill icons plus its wild
/// icons. Cards whose code isn't in the installed registry contribute
/// 0; no registry installed = 0 contribution overall.
fn sum_committed_icons(hand: &[CardCode], indices: &[u8], skill: SkillKind) -> i8 {
    let Some(reg) = card_registry::current() else {
        return 0;
    };
    indices
        .iter()
        .map(|&idx| {
            let code = &hand[usize::from(idx)];
            (reg.metadata_for)(code).map_or(0_i8, |meta| {
                let icons = meta.skill_icons();
                let matching = match skill {
                    SkillKind::Willpower => icons.willpower,
                    SkillKind::Intellect => icons.intellect,
                    SkillKind::Combat => icons.combat,
                    SkillKind::Agility => icons.agility,
                };
                let raw = matching.saturating_add(icons.wild);
                i8::try_from(raw).unwrap_or(i8::MAX)
            })
        })
        .fold(0_i8, i8::saturating_add)
}

/// Advance the RNG, draw a chaos token, compute the clamped total
/// against `difficulty`, and emit the per-step events
/// (`ChaosTokenRevealed` + either `SkillTestSucceeded` or
/// `SkillTestFailed`). Returns `true` on success so the caller can
/// branch its follow-up.
///
/// All arithmetic stays in `i8` with saturating ops: realistic
/// gameplay values (skill 1–8, modifier ±8, difficulty ≤ ~6) fit far
/// inside `i8`, but saturation defends against absurd state
/// configurations without needing a wider integer type.
fn resolve_chaos_token_and_emit(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    skill_value: i8,
) -> (bool, u8) {
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];

    // Symbol tokens may route to the active scenario's reference-card
    // effects (modifier + deferred side effects). Numeric/AutoFail/
    // ElderSign never do; nor do scenarios without a hook (static path).
    let symbol_outcome = match token {
        ChaosToken::Skull | ChaosToken::Cultist | ChaosToken::Tablet | ChaosToken::ElderThing => {
            crate::scenario::resolve_symbol_token(cx.state, token, investigator)
        }
        _ => None,
    };

    let resolution = match &symbol_outcome {
        Some(outcome) => TokenResolution::Modifier(outcome.modifier),
        None => resolve_token(token, &cx.state.token_modifiers),
    };
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && fail_reason.is_none();
    let failed_by = if succeeded {
        0
    } else {
        difficulty.saturating_sub(total)
    };
    if succeeded {
        cx.events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        cx.events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason,
            by: failed_by,
        });
    }

    // Symbol side effects resolve after success/failure is known.
    if let Some(outcome) = symbol_outcome {
        apply_symbol_outcome(cx, investigator, &outcome, succeeded);
    }

    (succeeded, u8::try_from(failed_by).unwrap_or(0))
}

/// Move every committed hand card to the controller's discard pile,
/// emitting [`Event::CardDiscarded`] for each. Per the
/// [`Event::SkillTestEnded`] docs, these discards precede the
/// `SkillTestEnded` cleanup marker. Walk indices in descending order
/// so each `remove` keeps the still-pending indices stable.
fn discard_committed_cards(cx: &mut Cx, investigator: InvestigatorId, indices_u8: &[u8]) {
    let mut sorted: Vec<u8> = indices_u8.to_vec();
    sorted.sort_by(|a, b| b.cmp(a));
    // Collect discarded codes first (releasing the mutable borrow on
    // cx.state) so we can push to cx.events afterwards without a
    // simultaneous borrow through cx.
    let discarded: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get_mut(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "discard_committed_cards: investigator {investigator:?} vanished after \
                     follow-up; this is a state-corruption invariant violation"
                )
            });
        sorted
            .iter()
            .map(|&idx| {
                let code = inv.hand.remove(usize::from(idx));
                inv.discard.push(code.clone());
                code
            })
            .collect()
    };
    for code in discarded {
        cx.events.push(Event::CardDiscarded {
            investigator,
            code,
            from: Zone::Hand,
        });
    }
}

/// Dispatch the action-specific on-success effect for the resolving
/// skill test. Failure-path follow-ups (none today) would route here
/// too if we grow them.
fn apply_skill_test_follow_up(
    cx: &mut Cx,
    investigator: InvestigatorId,
    follow_up: SkillTestFollowUp,
) -> EngineOutcome {
    match follow_up {
        SkillTestFollowUp::None => EngineOutcome::Done,
        SkillTestFollowUp::Investigate => {
            let effect = discover_clue(LocationTarget::YourLocation, 1);
            // discover_clue may suspend on a before-timing interrupt
            // (Cover Up 01007). Propagate AwaitingInput; the Investigate
            // follow-up has no source card, so `for_controller` is correct.
            // The only rejection path ("controller between locations")
            // can't occur post-Investigate (the action validated a
            // location), so a Rejected here is still an invariant violation.
            let eval_ctx = EvalContext::for_controller(investigator);
            let outcome = apply_effect(cx, &effect, eval_ctx);
            if let EngineOutcome::Rejected { reason } = &outcome {
                unreachable!(
                    "Investigate follow-up: discover_clue rejected unexpectedly after \
                     validation: {reason}"
                );
            }
            outcome
        }
        SkillTestFollowUp::Fight { enemy } => {
            // Mid-test enemy disappearance isn't possible in Phase 3
            // (no commit-window effects mutate enemies), so
            // damage_enemy's enemy-missing panic stays loud.
            super::combat::damage_enemy(cx, enemy, 1, Some(investigator));
            EngineOutcome::Done
        }
        SkillTestFollowUp::Evade { enemy } => {
            let e = cx.state.enemies.get_mut(&enemy).unwrap_or_else(|| {
                unreachable!(
                    "Evade follow-up: enemy {enemy:?} vanished while test was in flight; \
                     this is a state-corruption invariant violation"
                )
            });
            e.engaged_with = None;
            e.exhausted = true;
            cx.events.push(Event::EnemyDisengaged {
                enemy,
                investigator,
            });
            cx.events.push(Event::EnemyExhausted { enemy });
            EngineOutcome::Done
        }
    }
}

/// Fire a Retaliate attack if the just-resolved test was a *failed Fight*
/// against a ready enemy with the retaliate keyword.
///
/// Rules Reference p.18: *"Each time an investigator fails a skill test
/// while attacking a ready enemy with the retaliate keyword, after
/// applying all results for that skill test, that enemy performs an
/// attack against the attacking investigator. An enemy does not exhaust
/// after performing a retaliate attack."*
///
/// Runs at the `PostRetaliate` step — after `fire_on_skill_test_resolution`
/// (the rest of ST.7) and before the `PostOnResolution` teardown (ST.8) —
/// matching "after applying all results." The attack routes through
/// [`super::combat::enemy_attack`], which does not exhaust the attacker,
/// satisfying the no-exhaust clause for free.
///
/// No-op unless every condition holds: the test failed; its follow-up was
/// `Fight`; the enemy is still in play, ready (`!exhausted`), and has
/// `retaliate`. A missing enemy is skipped quietly — a failed fight deals
/// no damage, so the target can't have been defeated mid-test; this only
/// guards against future enemy-removing commit effects. This step is also
/// the future home of the "after an enemy attacks" reaction window (Guard
/// Dog C5b, Roland's reaction).
fn fire_retaliate_if_any(cx: &mut Cx, investigator: InvestigatorId, succeeded: bool) {
    if succeeded {
        return;
    }
    let follow_up = cx.state.in_flight_skill_test.as_ref().map(|t| t.follow_up);
    let Some(SkillTestFollowUp::Fight { enemy }) = follow_up else {
        return;
    };
    let retaliates = cx
        .state
        .enemies
        .get(&enemy)
        .is_some_and(|e| e.retaliate && !e.exhausted);
    if retaliates {
        super::combat::enemy_attack(cx, enemy, investigator);
    }
}

/// Fire `ForcedTriggerPoint::AfterLocationInvestigated` if the
/// just-resolved test was a *successful Investigate*. Runs at the
/// `PostOnResolution` step (after on-resolution triggers and retaliate,
/// "after applying all results"). No-op unless the test succeeded and
/// its follow-up was `Investigate`.
///
/// In-scope consumers (Obscuring Fog 01168 discards itself) neither
/// suspend nor produce 2+ simultaneous triggers, so a non-`Done`
/// outcome is a contract violation, surfaced loudly. Unlike
/// `fire_on_skill_test_resolution` (which only rejects on `Rejected`),
/// there is no resume path for a suspending consumer mid-teardown, so
/// any non-`Done` panics here. A suspending consumer is #212
/// reentrancy work.
fn fire_after_location_investigated(cx: &mut Cx, investigator: InvestigatorId, succeeded: bool) {
    if !succeeded {
        return;
    }
    let follow_up = cx.state.in_flight_skill_test.as_ref().map(|t| t.follow_up);
    if !matches!(follow_up, Some(SkillTestFollowUp::Investigate)) {
        return;
    }
    let Some(location) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|i| i.current_location)
    else {
        return;
    };
    let outcome = super::forced_triggers::fire_forced_triggers(
        cx,
        &super::forced_triggers::ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location,
        },
    );
    if !matches!(outcome, EngineOutcome::Done) {
        unreachable!(
            "AfterLocationInvestigated forced trigger returned non-Done ({outcome:?}); \
             slice-1 content (Obscuring Fog discards, no suspension / 2+ simultaneous). \
             A suspending consumer needs the #212 reentrancy work."
        );
    }
}

/// Iterate the active investigator's committed cards and fire each
/// matching [`Trigger::OnSkillTestResolution`] ability for the
/// resolved outcome.
///
/// Called inside `finish_skill_test` after the action-specific
/// [`SkillTestFollowUp`] has emitted its events and before the
/// committed cards discard. At evaluation time the cards are still in
/// hand at their hand indices and the in-flight record still holds
/// the tested location, so
/// [`LocationTarget::TestedLocation`] resolves cleanly.
///
/// **Rejections panic.** Card-impl bugs (e.g. an `OnSkillTestResolution`
/// effect that uses `LocationTarget::ChosenByController` without
/// `AwaitingInput` plumbing landing) are state-corruption invariant
/// violations once a card's been imported through the deck gate;
/// surface them loudly in tests rather than silently dropping the
/// triggered effect. Mirrors `apply_skill_test_follow_up`'s
/// `unreachable!` on a follow-up rejection.
fn fire_on_skill_test_resolution(
    cx: &mut Cx,
    investigator: InvestigatorId,
    indices_u8: &[u8],
    succeeded: bool,
) {
    let Some(reg) = card_registry::current() else {
        // No registry installed — engine-only tests that don't touch
        // card data don't reach OnSkillTestResolution at all. Silent
        // skip mirrors `constant_skill_modifier`'s behavior.
        return;
    };
    let outcome_now = if succeeded {
        crate::dsl::TestOutcome::Success
    } else {
        crate::dsl::TestOutcome::Failure
    };

    // Snapshot the (code, instance-eligible) pairs we'll iterate
    // before re-borrowing state mutably during apply_effect calls.
    // Each committed index resolves to a hand-position CardCode; the
    // cards are still in hand at this point (discard happens next).
    let codes: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "fire_on_skill_test_resolution: investigator {investigator:?} vanished while \
                 test was in flight; this is a state-corruption invariant violation"
                )
            });
        indices_u8
            .iter()
            .map(|&i| inv.hand[usize::from(i)].clone())
            .collect()
    };

    for code in &codes {
        let Some(abilities) = (reg.abilities_for)(code) else {
            continue;
        };
        for ability in abilities {
            let Trigger::OnSkillTestResolution { outcome } = ability.trigger else {
                continue;
            };
            if outcome != outcome_now {
                continue;
            }
            let eval_ctx = EvalContext::for_controller(investigator);
            let result = apply_effect(cx, &ability.effect, eval_ctx);
            if let EngineOutcome::Rejected { reason } = result {
                unreachable!(
                    "OnSkillTestResolution: effect for card {code:?} rejected unexpectedly: \
                     {reason}"
                );
            }
        }
    }
}

/// Public dispatch wrapper for [`PlayerAction::PerformSkillTest`].
///
/// Opens the commit window with no action-specific follow-up. The
/// after-resolution trigger window (#64) is downstream.
pub(super) fn perform_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    start_skill_test(
        cx,
        investigator,
        skill,
        SkillTestKind::Plain,
        difficulty,
        SkillTestFollowUp::None,
        None,
        None,
        None,
    )
}

pub(super) fn peril_check(
    _cx: &mut Cx,
    _code: &CardCode,
    _investigator: InvestigatorId,
    _is_peril: bool,
) {
    // TODO(future-peril-PR): if `is_peril`, install a temporary
    //   restriction on `_cx.state` such that other investigators cannot
    //   (a) play cards, (b) trigger abilities, or (c) commit to the
    //   drawing investigator's skill tests until this card's
    //   resolution completes.
}

/// Apply a resolved symbol token's side effects to the testing
/// investigator: `immediate` effects always, `on_fail` effects only when
/// the test failed. Routes through the same elimination paths as
/// `Effect::DealDamage` / `Effect::DealHorror`, so defeat handling and
/// the `DamageTaken` / `HorrorTaken` events are reused.
fn apply_symbol_outcome(
    cx: &mut Cx,
    investigator: InvestigatorId,
    outcome: &crate::scenario::SymbolOutcome,
    succeeded: bool,
) {
    use crate::scenario::TokenEffect;
    let mut effects: Vec<TokenEffect> = outcome.immediate.clone();
    if !succeeded {
        effects.extend(outcome.on_fail.iter().copied());
    }
    for effect in effects {
        match effect {
            TokenEffect::Damage(n) => {
                crate::engine::dispatch::elimination::take_damage(cx, investigator, n);
            }
            TokenEffect::Horror(n) => {
                crate::engine::dispatch::elimination::take_horror(cx, investigator, n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_event;
    use crate::assert_no_event;
    use crate::event::Event;
    use crate::scenario::{SymbolOutcome, TokenEffect};
    use crate::test_support::{test_investigator, GameStateBuilder};

    #[test]
    fn apply_symbol_outcome_runs_immediate_always_and_on_fail_only_on_failure() {
        let inv = InvestigatorId(1);

        let run = |succeeded: bool, outcome: SymbolOutcome| {
            let mut state = GameStateBuilder::new()
                .with_investigator(test_investigator(1))
                .build();
            let mut events = Vec::new();
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            apply_symbol_outcome(&mut cx, inv, &outcome, succeeded);
            events
        };

        // Case 1: immediate Damage(1), test succeeded → DamageTaken present.
        let ev = run(
            true,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![TokenEffect::Damage(1)],
                on_fail: vec![],
            },
        );
        assert_event!(ev, Event::DamageTaken { investigator, amount: 1 } if *investigator == inv);

        // Case 2: on_fail Horror(1), test succeeded → HorrorTaken absent.
        let ev = run(
            true,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![],
                on_fail: vec![TokenEffect::Horror(1)],
            },
        );
        assert_no_event!(ev, Event::HorrorTaken { .. });

        // Case 3: on_fail Horror(1), test failed → HorrorTaken present.
        let ev = run(
            false,
            SymbolOutcome {
                modifier: 0,
                immediate: vec![],
                on_fail: vec![TokenEffect::Horror(1)],
            },
        );
        assert_event!(ev, Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv);
    }

    /// A treachery-Revelation `Effect::SkillTest` (simulated via
    /// `start_skill_test` with an `on_fail` + the `pending_revelation_discard`
    /// slot `resolve_encounter_card` would set) suspends at the commit
    /// window, then on a failing draw deals the margin in damage and
    /// flushes the source treachery to `encounter_discard`.
    #[test]
    fn revelation_skill_test_failure_deals_margin_damage_and_discards() {
        use crate::dsl::{deal_damage, for_each_point_failed, InvestigatorTarget};
        use crate::state::{CardCode, ChaosToken};

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        // AutoFail forces the total to 0 → fail by `difficulty` (= 2).
        state.chaos_bag.tokens = vec![ChaosToken::AutoFail];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // What the evaluator's Effect::SkillTest arm does:
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            None,
            Some(for_each_point_failed(deal_damage(
                InvestigatorTarget::You,
                1,
            ))),
            None,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        // What resolve_encounter_card does on a suspended revelation:
        cx.state.pending_revelation_discard = Some(CardCode("01162".into()));

        // Resume: commit no cards → AutoFail → fail by 2 → 2 damage.
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&inv].damage, 2,
            "1 damage per point failed"
        );
        assert!(
            state.encounter_discard.contains(&CardCode("01162".into())),
            "suspended treachery flushed to encounter_discard at teardown"
        );
        assert!(state.in_flight_skill_test.is_none());
        assert!(state.pending_revelation_discard.is_none());
    }

    /// A plain (non-revelation) skill test never touches the
    /// `pending_revelation_discard` slot — the flush is a no-op for it.
    #[test]
    fn plain_skill_test_leaves_pending_revelation_discard_untouched() {
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = perform_skill_test(&mut cx, inv, SkillKind::Intellect, 1);
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert!(
            state.pending_revelation_discard.is_none(),
            "plain test must not set or flush the revelation-discard slot"
        );
        assert!(state.encounter_discard.is_empty());
    }

    /// A skill test with an `on_success` effect runs it on a successful
    /// draw (the success-side mirror of the `on_fail` path).
    #[test]
    fn skill_test_runs_on_success_effect_on_a_passing_draw() {
        use crate::dsl::{deal_horror, InvestigatorTarget};
        use crate::state::ChaosToken;

        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        // Willpower 3 + Numeric(0) = 3 vs difficulty 2 → success.
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = start_skill_test(
            &mut cx,
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
            SkillTestFollowUp::None,
            Some(deal_horror(InvestigatorTarget::You, 1)),
            None,
            None,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&inv].horror, 1,
            "on_success effect ran on the passing draw",
        );
    }
}
