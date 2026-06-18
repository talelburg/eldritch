//! Skill-test resolution handlers.
//!
//! Contains the full skill-test lifecycle: starting a test
//! ([`start_skill_test`]), the commit-stage entry ([`finish_skill_test`]),
//! the resolution driver ([`drive_skill_test`]), and all supporting
//! helpers.

use std::collections::{BTreeMap, BTreeSet};

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
use super::super::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use super::Cx;
use crate::action::InputResponse;

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
    test_modifier: i8,
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
        test_modifier,
        bonus_attack_damage: 0,
    });
    cx.events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    // "Use X in place of Y?" (Mind over Matter 01036): if this is a Combat or
    // Agility test and the investigator has a covering round-scoped
    // substitution active, offer the choice BEFORE the commit window — the
    // test type is fixed here (per the card's FAQ). The in-flight record (just
    // created) is the parking; `resume_substitution_choice` rewrites its skill
    // on "yes". Routed via `pending_substitution_prompt`.
    if substitution_covers(cx.state, investigator, skill) {
        cx.state.pending_substitution_prompt = Some(investigator);
        let use_skill = SkillKind::Intellect; // sole substitution in scope
        return EngineOutcome::AwaitingInput {
            request: InputRequest::choice(
                format!(
                    "{investigator:?}: use {use_skill:?} in place of {skill:?} for this test? \
                     (PickSingle(0) = use {use_skill:?}, PickSingle(1) = keep {skill:?})",
                ),
                vec![
                    ChoiceOption {
                        id: OptionId(0),
                        label: format!("Use {use_skill:?}"),
                    },
                    ChoiceOption {
                        id: OptionId(1),
                        label: format!("Keep {skill:?}"),
                    },
                ],
            ),
            resume_token: ResumeToken(0),
        };
    }
    open_commit_window(cx)
}

/// Whether `investigator` has an active round-scoped substitution covering a
/// `skill` test (Mind over Matter 01036: Intellect for Combat/Agility).
fn substitution_covers(state: &GameState, investigator: InvestigatorId, skill: SkillKind) -> bool {
    state
        .skill_substitutions
        .iter()
        .any(|s| s.investigator == investigator && s.for_skills.contains(&skill))
}

/// Push the skill-test resume frame and return the commit-window
/// `AwaitingInput` for the in-flight test. Shared by `start_skill_test` (the
/// no-substitution path) and `resume_substitution_choice` (after the Mind over
/// Matter prompt). Reads the (possibly rewritten) skill off the in-flight
/// record so the prompt message matches.
fn open_commit_window(cx: &mut Cx) -> EngineOutcome {
    let (investigator, skill, difficulty) = {
        let t = cx
            .state
            .in_flight_skill_test
            .as_ref()
            .expect("open_commit_window: in-flight test must exist");
        (t.investigator, t.skill, t.difficulty)
    };
    // Resume-handle on the one stack (Axis-B T4): the test parks at its commit
    // window. Resolution (reaction/fast) frames push *above* this when a window
    // opens mid-test; popped when the test fully resolves.
    cx.state
        .continuations
        .push(crate::state::Continuation::SkillTest);
    EngineOutcome::AwaitingInput {
        request: InputRequest::prompt(format!(
            "Commit cards from hand for {investigator:?}'s {skill:?} skill test \
             (difficulty {difficulty}). Empty indices commits no cards.",
        )),
        resume_token: ResumeToken(0),
    }
}

/// Resume the Mind over Matter substitution prompt (#322): `PickSingle(0)`
/// rewrites the in-flight test to an Intellect test (dropping any weapon combat
/// bonus per the FAQ "ignore bonuses to Combat or Agility"); `PickSingle(1)`
/// keeps the printed skill (a genuine "may" — a player may decline to fail on
/// purpose). Either way, opens the commit window.
pub(in crate::engine) fn resume_substitution_choice(
    cx: &mut Cx,
    response: &InputResponse,
) -> EngineOutcome {
    let InputResponse::PickSingle(OptionId(opt)) = response else {
        return EngineOutcome::Rejected {
            reason: "substitution prompt expects PickSingle(0|1)".into(),
        };
    };
    if *opt > 1 {
        return EngineOutcome::Rejected {
            reason: format!("substitution prompt: PickSingle({opt}) out of range (0|1)").into(),
        };
    }
    cx.state.pending_substitution_prompt = None;
    if *opt == 0 {
        // Use Intellect: the test becomes an Intellect test (base / icons /
        // bonuses all key off `skill`), and a weapon's combat bonus
        // (`test_modifier`) is dropped — FAQ "ignore any bonuses to Combat or
        // Agility". Bonus damage (`extra_damage` / `bonus_attack_damage`) is
        // separate and untouched.
        let t = cx
            .state
            .in_flight_skill_test
            .as_mut()
            .expect("resume_substitution_choice: in-flight test must exist");
        t.skill = SkillKind::Intellect;
        t.test_modifier = 0;
    }
    open_commit_window(cx)
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

    // Fire committed cards' OnCommit effects (Vicious Blow's attack buff)
    // before the test resolves — the commit step precedes resolution, and
    // a Fight follow-up's damage reads the accumulator they populate.
    fire_on_commit(cx, investigator, &indices_u8);

    let (succeeded, failed_by) =
        resolve_chaos_token_and_emit(cx, investigator, skill, difficulty, skill_value);

    // Build the eval context for the success/failure card effects,
    // threading the firing instance (`source`) so `Effect::DiscardSelf`
    // can find itself across the suspend/resume boundary.
    let card_ctx = |investigator: InvestigatorId| {
        EvalContext::for_controller_with_optional_source(investigator, source)
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
        // effects (Deal / Native) run to completion;
        // a future suspending on_fail is #212 reentrancy work.
        let mut ctx = card_ctx(investigator);
        ctx.failed_by = Some(failed_by);
        let outcome = apply_effect(cx, effect, ctx);
        if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
            // on_fail suspended on a controller choice (Crypt Chill 01167's
            // "choose an asset to discard", Axis A #334). The continuation is
            // already `PostFollowUp` (pre-advanced above), so resuming the
            // choice re-enters `drive_skill_test` at teardown — `on_fail`
            // does not re-run. Mirrors the follow-up-suspend path above.
            return outcome;
        }
        debug_assert!(
            matches!(outcome, EngineOutcome::Done),
            "revelation on_fail must resolve to Done or AwaitingInput: {outcome:?}"
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
        // Suspend only for a reaction window opened *during* this test — one
        // pushed *above* this test's `SkillTest` frame. A Resolution frame
        // *below* it is a forced run that fired this test as one of its
        // candidates (#213 reentrancy: two Frozen in Fear copies); it must
        // not be mistaken for a mid-test window — it resumes only once this
        // test fully tears down (via `resume_skill_test_commit`).
        let skill_test_pos = cx
            .state
            .continuations
            .iter()
            .rposition(|c| matches!(c, crate::state::Continuation::SkillTest));
        if let Some(win_idx) = cx.state.top_reaction_window_index() {
            if skill_test_pos.is_none_or(|st| win_idx > st) {
                return super::reaction_windows::open_queued_reaction_window(cx);
            }
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
            FinishContinuation::PostOnResolution { succeeded: _ } => {
                // "After you successfully investigate" (Obscuring Fog forced +
                // Dr. Milan reaction) already fired at the PostFollowUp step,
                // via the Investigate follow-up's `emit_event`
                // (`SuccessfullyInvestigated`, #213) — forced-before-reaction.
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
                // Remove this test's SkillTest resume-handle (Axis-B T4).
                // Usually it is the top frame, but a player-window gate can
                // legitimately sit above it (#69/#70/#71), so remove the
                // (unique — no nesting today) SkillTest frame by position
                // rather than popping the top.
                let frame = cx
                    .state
                    .continuations
                    .iter()
                    .rposition(|c| matches!(c, crate::state::Continuation::SkillTest));
                match frame {
                    Some(pos) => {
                        cx.state.continuations.remove(pos);
                    }
                    None => debug_assert!(
                        false,
                        "skill-test teardown: no SkillTest frame on the continuation stack",
                    ),
                }
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

    // Enforce per-card "Max N committed per skill test" caps (#311). The
    // limit is printed metadata (`CardKind::Skill.commit_limit`); a card
    // with no cap, or no registry entry, is unconstrained. No-op without an
    // installed registry — engine-only tests that don't touch card data
    // never commit real cards, mirroring `sum_skill_value`.
    if let Some(reg) = card_registry::current() {
        let mut counts: BTreeMap<&CardCode, u8> = BTreeMap::new();
        for &i in &indices_u8 {
            *counts.entry(&inv.hand[usize::from(i)]).or_insert(0) += 1;
        }
        for (code, count) in counts {
            let cap = (reg.metadata_for)(code).and_then(|m| match m.kind {
                crate::card_data::CardKind::Skill { commit_limit, .. } => commit_limit,
                _ => None,
            });
            if let Some(limit) = cap {
                if count > limit {
                    return Err(EngineOutcome::Rejected {
                        reason: format!(
                            "CommitCards: {code} allows at most {limit} committed per skill \
                             test, but {count} were committed"
                        )
                        .into(),
                    });
                }
            }
        }
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
    // One-shot modifier the initiating effect snapshotted (a weapon's
    // "+N for this attack"); 0 for player-action tests.
    let test_mod = state
        .in_flight_skill_test
        .as_ref()
        .map_or(0, |t| t.test_modifier);
    base.saturating_add(constant_mod)
        .saturating_add(pending_mod)
        .saturating_add(icon_mod)
        .saturating_add(test_mod)
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
            // This follow-up runs only on a successful Investigate, so the
            // "after you successfully investigate" timing point fires once the
            // discovery completes. Both its forced abilities (Obscuring Fog
            // 01168 discards) and its reaction window (Dr. Milan 01033) go
            // through one `emit_event` so the forced resolves *before* the
            // reaction window opens (RR p.2 forced-before-reaction, #213).
            // No-op when nothing matches, so a plain Investigate is unchanged.
            // A suspended discovery (Cover Up's before-interrupt, AwaitingInput)
            // resumes through PostFollowUp; firing across that boundary is out
            // of Slice-1 scope (Dr. Milan + Cover Up don't co-occur).
            if matches!(outcome, EngineOutcome::Done) {
                if let Some(location) = cx
                    .state
                    .investigators
                    .get(&investigator)
                    .and_then(|i| i.current_location)
                {
                    return super::emit::emit_event(
                        cx,
                        &super::emit::TimingEvent::SuccessfullyInvestigated {
                            investigator,
                            location,
                        },
                    );
                }
            }
            outcome
        }
        SkillTestFollowUp::Fight {
            enemy,
            extra_damage,
        } => {
            // Mid-test enemy disappearance isn't possible in Phase 3
            // (no commit-window effects mutate enemies), so
            // damage_enemy's enemy-missing panic stays loud. A weapon's
            // bonus damage (.38 Special's +1) rides on `extra_damage`; a
            // committed skill's bonus (Vicious Blow's +1) accumulates on
            // the in-flight record at commit time (#307). The in-flight
            // test is still present here — it's torn down only at the end
            // of resolution — so the accumulator is readable.
            let bonus = cx
                .state
                .in_flight_skill_test
                .as_ref()
                .map_or(0, |t| t.bonus_attack_damage);
            super::combat::damage_enemy(
                cx,
                enemy,
                1u8.saturating_add(extra_damage).saturating_add(bonus),
                Some(investigator),
            );
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
    let Some(SkillTestFollowUp::Fight { enemy, .. }) = follow_up else {
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
/// effect that uses `LocationTarget::Chosen` without
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

/// Iterate the active investigator's committed cards and fire each
/// [`Trigger::OnCommit`] ability's effect.
///
/// Called inside `finish_skill_test` after the commit indices are
/// validated and **before** the chaos token resolves — committing a card
/// happens before the test is resolved (Rules Reference: the commit step
/// precedes skill-test resolution). The in-scope consumer
/// ([`Effect::BoostAttackDamage`](crate::dsl::Effect::BoostAttackDamage),
/// Vicious Blow 01025) accumulates onto the in-flight record so the Fight
/// follow-up reads it; it never suspends.
///
/// **Rejections panic.** As with [`fire_on_skill_test_resolution`], an
/// `OnCommit` effect that returns non-`Done` is a card-impl bug — there is
/// no resume path mid-commit, so surface it loudly. A suspending commit
/// effect is #212 reentrancy work. No-op without an installed registry
/// (engine-only tests that don't touch card data never commit real cards).
fn fire_on_commit(cx: &mut Cx, investigator: InvestigatorId, indices_u8: &[u8]) {
    let Some(reg) = card_registry::current() else {
        return;
    };
    // Snapshot the committed hand codes before re-borrowing state mutably
    // during apply_effect calls. The cards are still in hand at commit.
    let codes: Vec<CardCode> = {
        let inv = cx
            .state
            .investigators
            .get(&investigator)
            .unwrap_or_else(|| {
                unreachable!(
                    "fire_on_commit: investigator {investigator:?} vanished while test was in \
                     flight; this is a state-corruption invariant violation"
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
            if !matches!(ability.trigger, Trigger::OnCommit) {
                continue;
            }
            let eval_ctx = EvalContext::for_controller(investigator);
            let result = apply_effect(cx, &ability.effect, eval_ctx);
            if let EngineOutcome::Rejected { reason } = result {
                unreachable!("OnCommit: effect for card {code:?} rejected unexpectedly: {reason}");
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
        0, // bare PerformSkillTest: no effect modifier
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
/// `Effect::Deal`, so defeat handling and the `DamageTaken` /
/// `HorrorTaken` events are reused.
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

    /// The `Fight` follow-up deals `1 + extra_damage + bonus_attack_damage`,
    /// reading the commit-time accumulator off the in-flight record
    /// (Vicious Blow 01025). With `extra_damage: 1` (a weapon bonus) and
    /// `bonus_attack_damage: 2`, the attack deals `1 + 1 + 2 = 4`.
    #[test]
    fn fight_follow_up_adds_bonus_attack_damage() {
        use crate::state::EnemyId;
        use crate::test_support::test_enemy;

        let inv = InvestigatorId(1);
        let mut enemy = test_enemy(7, "Goon");
        enemy.max_health = 10; // avoid clamping so the dealt damage is observable
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_enemy(enemy)
            .build();
        state.in_flight_skill_test = Some(InFlightSkillTest {
            investigator: inv,
            skill: SkillKind::Combat,
            kind: SkillTestKind::Fight,
            difficulty: 2,
            committed_by_active: Vec::new(),
            tested_location: None,
            follow_up: SkillTestFollowUp::Fight {
                enemy: EnemyId(7),
                extra_damage: 1,
            },
            on_fail: None,
            on_success: None,
            source: None,
            continuation: FinishContinuation::AwaitingCommit,
            test_modifier: 0,
            bonus_attack_damage: 2,
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        let out = apply_skill_test_follow_up(
            &mut cx,
            inv,
            SkillTestFollowUp::Fight {
                enemy: EnemyId(7),
                extra_damage: 1,
            },
        );

        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(7)].damage,
            4,
            "1 base + 1 extra_damage + 2 bonus_attack_damage"
        );
    }

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
            0,
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
            0,
        );
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        let out = finish_skill_test(&mut cx, &[]);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&inv].horror, 1,
            "on_success effect ran on the passing draw",
        );
    }

    fn substitution_state(inv: InvestigatorId) -> GameState {
        use crate::state::{ChaosToken, SkillSubstitution};
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
        state.skill_substitutions.push(SkillSubstitution {
            investigator: inv,
            use_skill: SkillKind::Intellect,
            for_skills: vec![SkillKind::Combat, SkillKind::Agility],
        });
        state
    }

    #[test]
    fn combat_test_with_substitution_prompts_then_becomes_intellect_on_yes() {
        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            // test_modifier 2 stands in for a weapon's +combat bonus.
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                2,
            )
        };
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }), "prompt");
        assert_eq!(state.pending_substitution_prompt, Some(inv));

        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(0)))
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit window"
        );
        let t = state.in_flight_skill_test.as_ref().unwrap();
        assert_eq!(t.skill, SkillKind::Intellect, "now an intellect test");
        assert_eq!(t.kind, SkillTestKind::Fight, "still a Fight (damage)");
        assert_eq!(t.test_modifier, 0, "weapon combat bonus dropped");
        assert!(state.pending_substitution_prompt.is_none());
    }

    #[test]
    fn substitution_choice_no_keeps_the_printed_skill() {
        let inv = InvestigatorId(1);
        let mut state = substitution_state(inv);
        let mut events = Vec::new();
        {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let _ = start_skill_test(
                &mut cx,
                inv,
                SkillKind::Agility,
                SkillTestKind::Evade,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                0,
            );
        }
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            resume_substitution_choice(&mut cx, &InputResponse::PickSingle(OptionId(1)))
        };
        assert!(
            matches!(out, EngineOutcome::AwaitingInput { .. }),
            "commit window"
        );
        assert_eq!(
            state.in_flight_skill_test.as_ref().unwrap().skill,
            SkillKind::Agility,
            "declined — keeps the printed skill",
        );
    }

    #[test]
    fn no_active_substitution_opens_commit_window_directly() {
        let inv = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(inv)
            .build();
        state.chaos_bag.tokens = vec![crate::state::ChaosToken::Numeric(0)];
        let mut events = Vec::new();
        let out = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            start_skill_test(
                &mut cx,
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                3,
                SkillTestFollowUp::None,
                None,
                None,
                None,
                0,
            )
        };
        assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
        assert!(state.pending_substitution_prompt.is_none(), "no prompt");
        assert_eq!(
            state.in_flight_skill_test.as_ref().unwrap().skill,
            SkillKind::Combat,
        );
    }
}
