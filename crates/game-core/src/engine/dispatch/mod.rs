//! Per-action dispatch handlers.
//!
//! Each function applies a single action variant to the state, mutating
//! the state in place and pushing the resulting events onto the events
//! buffer. Returns the [`EngineOutcome`] for the action.
//!
//! Handlers are split by `Action` bucket: [`apply_player_action`] for
//! human-initiated actions, [`apply_engine_record`] for engine-emitted
//! ones.

use crate::action::{EngineRecord, InputResponse, PlayerAction};
use crate::card_data::CardType;
use crate::state::CardCode;

use super::outcome::EngineOutcome;
use super::Cx;

mod abilities;
pub(crate) mod act_agenda;
mod actions;
// pub(super): engine/mod.rs re-exports `suspend_for_native_choice` (pub) for
// the `cards` crate's native-leaf picks (Crypt Chill 01167, Axis A #334).
pub(super) mod choice;
// pub(super): evaluator reaches grant_resources via the full path
// crate::engine::dispatch::cards::grant_resources (a sibling of dispatch).
pub(super) mod cards;
// pub(super): the unified trigger-dispatch chokepoint (Axis-B T5a); engine/mod.rs
// re-exports emit_event + TimingEvent via pub(crate) for the GameEnd site.
pub(super) mod emit;
// pub(crate): engine/mod.rs re-exports `deal_damage_to_enemy` for the
// `cards` crate (Guard Dog 01021's retaliate native, C5b #237).
pub(crate) mod combat;
mod cursor;
// pub(super): evaluator reaches take_damage/take_horror via the full path
// crate::engine::dispatch::elimination (a sibling of dispatch).
pub(super) mod elimination;
// pub(crate): engine/mod.rs re-exports `spawn_set_aside_enemy` for the
// `cards` crate (The Gathering's Act-2 reverse).
pub(crate) mod encounter;
// pub(super): engine/mod.rs re-exports ForcedTriggerPoint + fire_forced_triggers
// via pub(crate) for test_support::fire_forced_at (Task 2 of #215).
pub(super) mod forced_triggers;
pub(crate) mod hunters;
pub(super) mod phases;
// `pub(super)` so the evaluator's `discover_clue` can open the Before-discover
// window via the `pub(crate)` `open_queued_reaction_window` (Axis D #336);
// other items stay `pub(super)`-to-dispatch.
pub(super) mod reaction_windows;
pub(crate) mod reveal;
// pub(super): engine::evaluator reaches start_skill_test for Effect::SkillTest.
pub(super) mod skill_test;
pub(crate) mod threat_area;

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// Phase-1 minimal coverage: [`StartScenario`](PlayerAction::StartScenario)
/// and [`EndTurn`](PlayerAction::EndTurn) are implemented end-to-end;
/// other variants return [`EngineOutcome::Rejected`] with a TODO message
/// so callers and tests get a useful signal rather than a silent no-op.
#[allow(clippy::too_many_lines)] // dispatcher: a guard ladder + one match arm per PlayerAction
pub fn apply_player_action(cx: &mut Cx, action: &PlayerAction) -> EngineOutcome {
    // A pending prompt gates every action but `ResolveInput` (slice 1b, #393).
    // After the §1 continuation-stack work and the phase-anchor slices, the
    // frame awaiting input is always the top of the stack, and *every* non-anchor
    // frame on top is such a prompt — reaction/Fast window, skill-test commit,
    // choice, substitution prompt, hunter/spawn pick, hand-size discard, act
    // round-end, mulligan, encounter draw. A `*Phase` anchor on top is the open
    // turn (or inert), so typed actions are allowed there. This single rule
    // replaces the former eight per-suspension guard blocks; the specific
    // expected `InputResponse` rides the `AwaitingInput` request the client
    // already holds.
    if cx
        .state
        .continuations
        .last()
        .is_some_and(crate::state::Continuation::awaits_input)
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a prompt is outstanding; submit a PlayerAction::ResolveInput with the \
                     InputResponse the AwaitingInput request describes (PickSingle / \
                     PickMultiple / Confirm / Skip) before any other action"
                .into(),
        };
    }

    let outcome = match action {
        PlayerAction::StartScenario { roster } => phases::start_scenario(cx, roster),
        PlayerAction::EndTurn => phases::end_turn(cx),
        PlayerAction::PerformSkillTest {
            investigator,
            skill,
            difficulty,
        } => skill_test::perform_skill_test(cx, *investigator, *skill, *difficulty),
        PlayerAction::Investigate { investigator } => actions::investigate(cx, *investigator),
        PlayerAction::Resource { investigator } => actions::resource_action(cx, *investigator),
        PlayerAction::Engage {
            investigator,
            enemy,
        } => actions::engage(cx, *investigator, *enemy),
        PlayerAction::Move {
            investigator,
            destination,
        } => actions::move_action(cx, *investigator, *destination),
        PlayerAction::Draw { investigator } => cards::draw(cx, *investigator),
        PlayerAction::Fight {
            investigator,
            enemy,
        } => actions::fight(cx, *investigator, *enemy),
        PlayerAction::Evade {
            investigator,
            enemy,
        } => actions::evade(cx, *investigator, *enemy),
        PlayerAction::PlayCard {
            investigator,
            hand_index,
        } => cards::play_card(cx, *investigator, *hand_index),
        PlayerAction::ActivateAbility {
            investigator,
            instance_id,
            ability_index,
        } => abilities::activate_ability(cx, *investigator, *instance_id, *ability_index),
        PlayerAction::ResolveInput { response } => resolve_input(cx, response),
        PlayerAction::AdvanceAct { investigator } => {
            act_agenda::advance_act_action(cx, *investigator)
        }
    };

    // The post-mulligan Investigation kickoff moved into `resume_mulligan`
    // (#348): the mulligan loop now drains through `ResolveInput`, and
    // `resume_mulligan` begins the Investigation phase itself once the last
    // investigator has mulliganed. No outer-boundary kickoff remains here.

    // Reaction windows open at the step boundary inside the handler
    // that queued them (see `drive_skill_test`), not at this outer
    // boundary — the Rules Reference clause "after… may be used
    // immediately after that triggering condition's impact upon the
    // game state has resolved" is mid-action, not post-action. Any
    // future action that queues a window outside the skill-test
    // driver must add its own boundary check; there's no fallback
    // here.

    // Run the main loop (slice 1b, #393): advance any `*Phase` anchor a handler
    // left on top (a phase transition), carrying the cascade forward until it
    // blocks on a suspension, idles at the open turn, or reaches terminal.
    drive(cx, outcome)
}

/// The uniform main loop (slice 1b, #393). Given the action's `outcome`,
/// advance the top continuation frame until the engine blocks or idles:
///
/// - non-`Done` `outcome` (a suspension / rejection from the action itself)
///   passes straight through;
/// - a `*Phase` anchor on top (other than the open turn) is advanced via
///   [`phases::anchor_on_child_pop`], which runs its resume-keyed chunk and,
///   at a phase boundary, transitions by popping itself + pushing the next
///   phase's anchor (`Entry`) — the loop then advances that;
/// - the loop stops with `AwaitingInput` when an advance suspends, and with
///   `Done` at the open turn (`InvestigationPhase{TurnBegins}`), at terminal
///   (empty stack), or when an advance makes no progress (a parked phase, e.g.
///   Investigation with no active investigator).
pub(super) fn drive(cx: &mut Cx, outcome: EngineOutcome) -> EngineOutcome {
    if !matches!(outcome, EngineOutcome::Done) {
        return outcome;
    }
    loop {
        let top = cx.state.continuations.last().cloned();
        match top {
            Some(ref c) if c.is_phase_anchor() && !c.is_open_turn() => {
                match phases::anchor_on_child_pop(cx) {
                    EngineOutcome::Done => {
                        // No-progress guard: a parked phase (e.g. Investigation
                        // with no active investigator) leaves the same anchor on
                        // top — break rather than spin.
                        if cx.state.continuations.last() == top.as_ref() {
                            return EngineOutcome::Done;
                        }
                    }
                    other => return other,
                }
            }
            // Open turn idle, terminal (empty), or a suspension on top (which a
            // handler already surfaced as AwaitingInput before reaching here).
            _ => return EngineOutcome::Done,
        }
    }
}

/// Apply an [`EngineRecord`] to the state, pushing events.
pub fn apply_engine_record(cx: &mut Cx, record: &EngineRecord) -> EngineOutcome {
    match record {
        EngineRecord::DeckShuffled { investigator } => cards::deck_shuffled(cx, *investigator),
        EngineRecord::EncounterDeckShuffled => encounter::encounter_deck_shuffled(cx),
        EngineRecord::EncounterCardRevealed { investigator } => {
            encounter::encounter_card_revealed(cx, *investigator)
        }
    }
}

/// Internal helper: where a played card lands after on-play effects
/// resolve. Mirrors the Arkham rule that assets stay in play while
/// events resolve and go to the discard.
#[derive(Debug)]
pub(super) enum PlayDestination {
    /// Card stays in play (asset).
    InPlay,
    /// Card moves to the discard after on-play effects resolve (event).
    Discard,
}

/// Validated payload returned by [`check_play_card`] on success.
/// Carries the data `play_card`'s mutation step needs without
/// re-running the validation.
///
/// `is_fast` is consumed by [`any_fast_play_eligible`]; `card_type`
/// is currently destructured with `_` in `play_card` but kept for
/// future consumers (e.g. reaction-window dispatch).
///
/// `#[allow(dead_code)]` covers `card_type` (not yet read outside
/// validation) and suppresses the rustc `dead_code` lint on struct fields
/// that are only read by a `pub(super)` function not yet wired up.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) struct PlayCheckResult {
    pub destination: PlayDestination,
    pub abilities: Vec<crate::dsl::Ability>,
    pub is_fast: bool,
    pub card_type: CardType,
}

/// Validated payload returned by [`check_activate_ability`] on success.
/// Carries the data `activate_ability`'s mutation step needs without
/// re-running the validation.
#[derive(Debug)]
#[allow(dead_code)] // Fields consumed by any_fast_play_eligible in T05.
pub(super) struct ActivateCheckResult {
    /// Position of the source card in the investigator's `cards_in_play`.
    pub in_play_pos: usize,
    /// The card code of the source card.
    pub source_code: CardCode,
    /// Action cost from the ability's `Trigger::Activated`.
    pub action_cost: u8,
    /// Payment costs (beyond the action cost).
    pub costs: Vec<crate::dsl::Cost>,
    /// The effect to dispatch after paying costs.
    pub effect: crate::dsl::Effect,
    /// Whether the source card was exhausted at validation time —
    /// load-bearing for activated abilities whose payment includes
    /// `Cost::Exhaust`.
    pub source_exhausted: bool,
}

/// Resume the open window at the top of the stack: drive its reaction
/// triggers if any are pending, else close the pure-Fast window on `Skip`.
fn resume_window(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    // If the window has pending reaction triggers, drive the reaction
    // window; otherwise it is a pure-Fast window (empty `pending_triggers`)
    // that `Skip` closes.
    if cx.state.top_reaction_window().is_some() {
        return reaction_windows::resume_reaction_window(cx, response);
    }
    if matches!(response, InputResponse::Skip) {
        let idx = cx.state.continuations.len() - 1;
        return reaction_windows::close_reaction_window_at(cx, idx);
    }
    EngineOutcome::Rejected {
        reason: format!(
            "ResolveInput: a Fast-play window is open (no pending triggers); \
             submit InputResponse::Skip to close it, got {response:?}",
        )
        .into(),
    }
}

/// Resume a skill test parked at its commit window: the active investigator
/// submits their commit list via [`InputResponse::PickMultiple`] (each
/// [`OptionId`](crate::engine::OptionId) is a hand index).
fn resume_skill_test_commit(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    match response {
        InputResponse::PickMultiple { selected } => {
            let indices: Vec<u32> = selected.iter().map(|o| o.0).collect();
            let outcome = skill_test::finish_skill_test(cx, &indices);
            if matches!(outcome, EngineOutcome::Done) {
                // The resolved test was a sibling fired by a forced run (2+
                // simultaneous `EndOfTurn` forced — two Frozen in Fear copies,
                // #213). The run's frame is now back on top; re-enter it to
                // fire the remaining siblings, or close it (running its
                // continuation, e.g. end-of-turn rotation). Checked before
                // `pending_end_turn`: a forced run owns its own post-run
                // continuation and never sets `pending_end_turn`.
                if matches!(
                    cx.state.continuations.last(),
                    Some(crate::state::Continuation::Resolution(f)) if f.is_forced()
                ) {
                    let idx = cx.state.continuations.len() - 1;
                    return reaction_windows::advance_resolution(cx, idx);
                }
                // Otherwise: a single suspending `EndOfTurn` forced effect
                // (one Frozen in Fear) stranded `end_turn` before rotation;
                // resume it now that the test is fully done (C4c, #235). An
                // `AwaitingInput` mid-teardown leaves `pending_end_turn` set
                // for the next resume.
                if let Some(active_id) = cx.state.pending_end_turn.take() {
                    return phases::resume_end_turn(cx, active_id);
                }
            }
            outcome
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit window expects InputResponse::PickMultiple, \
                 got {other:?}",
            )
            .into(),
        },
    }
}

/// Dispatch a [`PlayerAction::ResolveInput`].
///
/// Routes to the right resume handler based on which suspension is
/// outstanding: an open reaction window ([`resume_reaction_window`])
/// or the skill-test commit window ([`finish_skill_test`]). Rejects
/// when nothing is outstanding.
///
/// A reaction window on `state.open_windows` and `in_flight_skill_test`
/// may both be present simultaneously — that's the mid-skill-test
/// reaction case: the skill-test driver is parked at a step boundary
/// waiting for the reaction window to close before continuing. The
/// reaction window takes routing priority; once it closes,
/// [`close_reaction_window_at`] re-enters [`drive_skill_test`] to finish
/// the test.
///
/// # Pure-Fast window closing
///
/// A pure-Fast window (pushed by [`open_fast_window`], empty
/// `pending_triggers`) is **not** returned by [`GameState::top_reaction_window`]
/// because that helper filters out empty-`pending_triggers` windows.
/// When such a window is the only entry on the stack (no
/// reaction-driven window below it), `InputResponse::Skip` closes it
/// directly via [`close_reaction_window_at`] on the literal top-of-stack
/// index. This covers the `MythosAfterDraws` window after all Fast
/// plays have been made and the player is done.
pub(crate) fn resolve_input(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    // Top-frame dispatch (umbrella §1 / #348): every suspension is a
    // `Continuation` frame, and the frame awaiting input is always the top of
    // the stack (each suspension pushes above whatever it suspended within — a
    // `SubstitutionPrompt` above its `SkillTest`, a reaction `Resolution` above
    // a mid-test commit, etc.). So routing is "dispatch on the top frame's
    // variant"; the former hand-ordered `if pending_X.is_some()` priority
    // cascade is gone.
    use crate::state::Continuation;
    let outcome = match cx.state.continuations.last() {
        Some(Continuation::SubstitutionPrompt { .. }) => {
            skill_test::resume_substitution_choice(cx, response)
        }
        Some(Continuation::Resolution(_)) => resume_window(cx, response),
        Some(Continuation::Choice(_)) => choice::resume_choice(cx, response),
        Some(Continuation::HunterMove(_)) => hunters::resume_hunter_choice(cx, response),
        Some(Continuation::SpawnEngage(_)) => hunters::resume_spawn_engage(cx, response),
        Some(Continuation::HandSizeDiscard(_)) => phases::resume_hand_size_discard(cx, response),
        Some(Continuation::ActRoundEnd(_)) => phases::resume_act_round_end_advance(cx, response),
        Some(Continuation::Mulligan { .. }) => cards::resume_mulligan(cx, response),
        Some(Continuation::EncounterDraw { .. }) => encounter::resume_encounter_draw(cx, response),
        // An `EncounterCard` frame never awaits input — it only ever sits
        // beneath a real suspension. If it is somehow top, no prompt is
        // outstanding (defensive; #380).
        Some(Continuation::EncounterCard { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (encounter-card disposal is \
                     framework-internal)"
                .into(),
        },
        Some(Continuation::SkillTest(_)) => resume_skill_test_commit(cx, response),
        // The open turn does not emit an AwaitingInput prompt in 2a (typed
        // actions drive it; the enumeration is 2a-ii / surfacing is 2b). A
        // ResolveInput arriving here is spurious — reject defensively, mirroring
        // the phase-anchor arm (slice 2a-i, #393).
        Some(Continuation::InvestigatorTurn { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (the open turn \
                     takes typed actions, not ResolveInput)"
                .into(),
        },
        // Phase anchors (slice 1a, #393) never await input — they only sit
        // beneath framework windows. If one is somehow top, no prompt is
        // outstanding (defensive, mirrors the EncounterCard arm).
        Some(
            Continuation::MythosPhase { .. }
            | Continuation::InvestigationPhase { .. }
            | Continuation::EnemyPhase { .. }
            | Continuation::UpkeepPhase { .. },
        ) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a phase anchor is top)".into(),
        },
        None => EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        },
    };
    // A treachery Revelation that suspended parks its `EncounterCard` frame
    // beneath the suspension (#380); once that sub-resolution completes (`Done`)
    // the frame is top again, so dispose of the card here — one generic site,
    // no resume handler aware of treacheries.
    if matches!(outcome, EngineOutcome::Done) {
        return encounter::teardown_encounter_card_if_top(cx);
    }
    outcome
}
