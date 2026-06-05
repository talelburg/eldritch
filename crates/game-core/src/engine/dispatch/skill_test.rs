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
    resolve_token, CardCode, FinishContinuation, GameState, InFlightSkillTest, InvestigatorId,
    SkillKind, SkillTestFollowUp, Status, TokenResolution, Zone,
};

use super::super::evaluator::{
    apply_effect, constant_skill_modifier, pending_skill_modifier, EvalContext,
};
use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

pub(super) fn start_skill_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty: i8,
    follow_up: SkillTestFollowUp,
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

    let succeeded = resolve_chaos_token_and_emit(cx, investigator, skill, difficulty, skill_value);

    if succeeded {
        apply_skill_test_follow_up(cx, investigator, follow_up);
    }

    // Step 2 is complete. Advance the continuation (carrying the
    // outcome forward) and let the driver handle the remaining
    // steps (including the possibly-queued reaction window from
    // inside the follow-up).
    cx.state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .continuation = FinishContinuation::PostFollowUp { succeeded };

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
                    .continuation = FinishContinuation::PostOnResolution { succeeded };
            }
            FinishContinuation::PostOnResolution { succeeded: _ } => {
                discard_committed_cards(cx, investigator, &indices_u8);
                cx.events.push(Event::SkillTestEnded { investigator });
                // ModifierScope::ThisSkillTest contributions expire when
                // the test ends. Drain pending entries for *this*
                // investigator only — entries queued for other
                // investigators' future tests stay.
                cx.state
                    .pending_skill_modifiers
                    .retain(|m| m.investigator != investigator);
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
                let matching = match skill {
                    SkillKind::Willpower => meta.skill_icons.willpower,
                    SkillKind::Intellect => meta.skill_icons.intellect,
                    SkillKind::Combat => meta.skill_icons.combat,
                    SkillKind::Agility => meta.skill_icons.agility,
                };
                let raw = matching.saturating_add(meta.skill_icons.wild);
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
) -> bool {
    let token_idx = cx.state.rng.next_index(cx.state.chaos_bag.tokens.len());
    let token = cx.state.chaos_bag.tokens[token_idx];
    let resolution = resolve_token(token, &cx.state.token_modifiers);
    cx.events
        .push(Event::ChaosTokenRevealed { token, resolution });

    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && fail_reason.is_none();
    if succeeded {
        cx.events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        let by = difficulty.saturating_sub(total);
        cx.events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason,
            by,
        });
    }
    succeeded
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
) {
    match follow_up {
        SkillTestFollowUp::None => {}
        SkillTestFollowUp::Investigate => {
            let effect = discover_clue(LocationTarget::ControllerLocation, 1);
            let eval_ctx = EvalContext::for_controller(investigator);
            // Same caveat as the pre-refactor `investigate`: the only
            // remaining rejection path inside `discover_clue` is
            // "controller is between locations", which the Investigate
            // action validates before starting the test. Empty-
            // location is a silent no-op by design. Any rejection
            // here is a state-corruption invariant violation.
            let outcome = apply_effect(cx, &effect, eval_ctx);
            if let EngineOutcome::Rejected { reason } = outcome {
                unreachable!(
                    "Investigate follow-up: discover_clue rejected unexpectedly after \
                     validation: {reason}"
                );
            }
        }
        SkillTestFollowUp::Fight { enemy } => {
            // Mid-test enemy disappearance isn't possible in Phase 3
            // (no commit-window effects mutate enemies), so
            // damage_enemy's enemy-missing panic stays loud.
            super::combat::damage_enemy(cx, enemy, 1, Some(investigator));
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
        }
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
