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
pub(crate) mod actions;
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
pub(super) mod coordinator;
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
pub(crate) mod reaction_windows;
pub(crate) mod reveal;
// pub(super): engine::evaluator reaches start_skill_test for Effect::SkillTest.
pub(super) mod skill_test;
pub(crate) mod threat_area;

/// Dispatch one enumerated open-turn action (the internal id→action map target).
/// The same handlers `apply_player_action`'s typed arms call; behaviour-identical.
/// Called from the `InvestigatorTurn { ending: false }` arm of `resolve_input`
/// (slice 2b, #447).
pub(crate) fn dispatch_turn_action(
    cx: &mut Cx,
    action: &crate::engine::enumerate::TurnAction,
) -> EngineOutcome {
    use crate::engine::enumerate::TurnAction;
    match action {
        TurnAction::EndTurn => phases::end_turn(cx),
        TurnAction::Move {
            investigator,
            destination,
        } => actions::move_action(cx, *investigator, *destination),
        TurnAction::Investigate { investigator } => actions::investigate(cx, *investigator),
        TurnAction::Resource { investigator } => actions::resource_action(cx, *investigator),
        TurnAction::Draw { investigator } => cards::draw(cx, *investigator),
        TurnAction::Fight {
            investigator,
            enemy,
        } => actions::fight(cx, *investigator, *enemy),
        TurnAction::Evade {
            investigator,
            enemy,
        } => actions::evade(cx, *investigator, *enemy),
        TurnAction::Engage {
            investigator,
            enemy,
        } => actions::engage(cx, *investigator, *enemy),
        TurnAction::PlayCard {
            investigator,
            hand_index,
        } => cards::play_card(cx, *investigator, *hand_index),
        TurnAction::ActivateAbility {
            investigator,
            instance_id,
            ability_index,
        } => abilities::activate_ability(cx, *investigator, *instance_id, *ability_index),
        TurnAction::AdvanceAct { investigator } => {
            act_agenda::advance_act_action(cx, *investigator)
        }
    }
}

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
    // that queued them (see `advance`), not at this outer
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
/// - a `*Phase` anchor on top is advanced via
///   [`phases::anchor_on_child_pop`], which runs its resume-keyed chunk and,
///   at a phase boundary, transitions by popping itself + pushing the next
///   phase's anchor (`Entry`) — the loop then advances that;
/// - an [`ActionResolution`](crate::state::Continuation::ActionResolution) frame
///   on top is resumed via [`resume_action_resolution`], which runs the
///   action's primary effect (or suppresses it if the actor was defeated);
/// - the loop stops with `AwaitingInput` when an advance suspends, and with
///   `Done` when an [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn)
///   frame is on top (the open turn — slice 2a-i, #393), at terminal (empty
///   stack), or when an advance makes no progress (a parked phase, e.g.
///   Investigation with no active investigator).
pub(crate) fn drive(cx: &mut Cx, outcome: EngineOutcome) -> EngineOutcome {
    use crate::state::Continuation;
    if !matches!(outcome, EngineOutcome::Done) {
        return outcome;
    }
    loop {
        let top = cx.state.continuations.last().cloned();
        match top {
            Some(ref c) if c.is_phase_anchor() => {
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
            Some(Continuation::ActionResolution { .. }) => {
                match resume_action_resolution(cx) {
                    EngineOutcome::Done => {
                        // Primary ran (or was suppressed) + frame popped; loop
                        // on — the InvestigatorTurn frame beneath is now top.
                    }
                    other => return other, // primary effect suspended (e.g. skill test)
                }
            }
            // An effect-walk frame parked across an `apply()` boundary (#422):
            // e.g. an on-play effect that opened a reaction window now resumes
            // after the window closed. Step it via the shared effect driver.
            Some(Continuation::Effect(_)) => {
                match crate::engine::evaluator::step_effect_frame(cx) {
                    EngineOutcome::Done => {
                        // Stepped (child pushed / frame popped); loop on.
                    }
                    other => return other, // suspended for a pick, or rejected
                }
            }
            // A window on top (Slice C-plumbing): advance one resume step —
            // re-prompt the next candidate, or (empty) close + run its
            // continuation. A `TimingPointWindow` is always dispatched (its
            // candidates are exhausted only by firing, so empty ⇒ close); an empty
            // `FastWindow` is a permissive Fast-gate awaiting `Skip` and is left to
            // idle below. Operates on the top frame — the invariant is that
            // `last()` is what resolves next, so no reach-down index.
            //
            // The guard: `TimingPointWindow` matches the first disjunct (always
            // dispatched). A `FastWindow`'s candidates are empty today (it is a
            // pure Fast-gate — `open_fast_window` pushes `Vec::new()`), so
            // `awaits_input()` is false and it idles; the `|| awaits_input()` arm
            // is the (currently dormant) path that would dispatch a candidate-
            // bearing framework window if one is ever added.
            Some(
                ref c @ (Continuation::TimingPointWindow { .. } | Continuation::FastWindow { .. }),
            ) if matches!(c, Continuation::TimingPointWindow { .. }) || c.awaits_input() => {
                match reaction_windows::advance_resolution(cx) {
                    EngineOutcome::Done => {} // closed; loop on to the exposed frame
                    other => return other,    // re-prompt, or a suspended continuation
                }
            }
            // A skill test re-exposed on top (a mid-test window/effect closed):
            // step its driver. By the invariant it is top — no `rposition` /
            // `win_idx > st` self-location.
            Some(Continuation::SkillTest(_)) => match skill_test::advance(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // An encounter-card frame re-exposed after its Revelation's
            // sub-resolution completed: dispose of the card (treachery discard /
            // enemy spawn) + pop (#380). `dispose_…` pops the frame, so the top
            // changes (exposing the drawer's `PlayerDraw` for the Mythos chain, or
            // a non-draw frame) and the loop makes progress; an enemy spawn can
            // suspend on an engagement tie, so a non-`Done` outcome propagates.
            Some(Continuation::EncounterCard { .. }) => {
                match encounter::dispose_encounter_card_if_top(cx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
            // A hand-play disposal frame re-exposed after its OnPlay effect
            // resolved: dispose of the card (event → flush pending_played_event;
            // asset → remove from hand, enter play, emit EnteredPlay) and pop
            // (Slice D #423). Never suspends itself — any reaction window queued by
            // emit_event lands on top and the loop drives it next.
            Some(Continuation::PlayFromHand { .. }) => match cards::dispose_play_from_hand(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // A per-drawer Mythos surge-chain frame (callsite-migration): draw
            // the next card (first step or a pending surge), or — chain over —
            // pop itself and advance the loop to the next drawer / post-1.4
            // window. Re-exposed by an `EncounterCard` disposal or a `SpawnEngage`
            // resume. A draw can suspend on an engagement tie, so a non-`Done`
            // outcome propagates.
            Some(Continuation::PlayerDraw { .. }) => match encounter::drive_player_draw(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // The `when → at → after` coordinator frames (#434). `EmitEvent`
            // walks the buckets (pushing a `TimingPoint` per populated cell);
            // `TimingPoint` runs one bucket's forced-then-reaction. Each does one
            // step and returns `Done` (loop re-dispatches the mutated top) or
            // `AwaitingInput` (a window / forced run opened). Only `RoundEnded`
            // uses them today (the round-end `when` advance + `at` doom).
            Some(Continuation::EmitEvent { .. }) => match coordinator::dispatch_emit_event(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            Some(Continuation::TimingPoint { .. }) => {
                match coordinator::dispatch_timing_point(cx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
            // The open turn is ending: a suspending `EndOfTurn` forced (a single
            // skill test, or a 2+ forced run) stranded `end_turn` before rotation
            // and flagged this frame. Re-exposed on top now that the suspension
            // resolved, drive the rotation tail. `ending: false` stays the idle
            // open-turn sentinel (the `_` arm below). Unifies the former two
            // resume paths (the skill-test reach-down + `EndOfTurnAfterForced`).
            Some(Continuation::InvestigatorTurn {
                investigator,
                ending: true,
            }) => match phases::resume_end_turn(cx, investigator) {
                EngineOutcome::Done => {} // rotated / phase ended; loop on
                other => return other,
            },
            // Idle: the open turn (an `InvestigatorTurn { ending: false }`
            // frame), an empty `FastWindow` permissive gate, terminal (empty), or
            // a suspension on top (which a handler already surfaced as
            // AwaitingInput).
            _ => return EngineOutcome::Done,
        }
    }
}

/// Resume a parked [`ActionResolution`](crate::state::Continuation::ActionResolution)
/// frame (#293): pop it, run the §D re-validation gate, then dispatch to the
/// action's primary effect. The gate suppresses the primary (returns `Done`,
/// leaving the spent action + AoO/window effects in place) if the actor was
/// defeated mid-action; each primary effect additionally re-checks its own
/// target precondition. Called only by [`drive`] with such a frame on top.
fn resume_action_resolution(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{ActionResume, Continuation};
    let Some(Continuation::ActionResolution {
        investigator,
        resume,
    }) = cx.state.continuations.pop()
    else {
        unreachable!("resume_action_resolution: top frame is not an ActionResolution");
    };
    // §D re-validation: actor still Active? If not, suppress the primary.
    let active = cx
        .state
        .investigators
        .get(&investigator)
        .is_some_and(|inv| inv.status == crate::state::Status::Active);
    if !active {
        // A defeated actor suppresses the primary effect, but a mid-play event
        // that already left hand (stashed in `pending_played_event` by
        // `begin_event_play`) must still be placed in discard (RR Appendix I
        // step 4: the card was "played" the moment it left hand; the suppression
        // only skips the `OnPlay` effect, not the discard). The
        // `PlayFromHand` frame won't run, so flush it here.
        cards::flush_pending_played_event(cx);
        return EngineOutcome::Done;
    }
    match resume {
        ActionResume::Move { destination } => {
            actions::move_primary_effect(cx, investigator, destination)
        }
        ActionResume::Investigate => actions::investigate_primary_effect(cx, investigator),
        ActionResume::Resource => actions::resource_primary_effect(cx, investigator),
        ActionResume::Engage { enemy } => actions::engage_primary_effect(cx, investigator, enemy),
        ActionResume::Draw => cards::draw_primary_effect(cx, investigator),
        ActionResume::ActivateAbility {
            instance_id,
            effect,
        } => abilities::resume_activate_ability(cx, investigator, instance_id, &effect),
        ActionResume::PlayCard { hand_index, code } => {
            cards::resume_play_card(cx, investigator, hand_index, &code)
        }
    }
}

/// Apply an [`EngineRecord`] to the state, pushing events.
///
/// Runs the main [`drive`] loop at the tail (mirroring [`apply_player_action`],
/// #423): `EncounterCardRevealed` now pushes a [`Continuation::EncounterCard`]
/// disposition frame plus the card's Revelation effect frames for the loop to
/// step, rather than resolving synchronously.
pub fn apply_engine_record(cx: &mut Cx, record: &EngineRecord) -> EngineOutcome {
    let outcome = match record {
        EngineRecord::DeckShuffled { investigator } => cards::deck_shuffled(cx, *investigator),
        EngineRecord::EncounterDeckShuffled => encounter::encounter_deck_shuffled(cx),
        EngineRecord::EncounterCardRevealed { investigator } => {
            encounter::encounter_card_revealed(cx, *investigator)
        }
    };
    drive(cx, outcome)
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
pub(crate) struct PlayCheckResult {
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
    // The top frame is the window the player is acting on (resolve_input routed
    // here for a `TimingPointWindow`/`FastWindow` on top). If it has pending
    // candidates, drive it; otherwise it is a pure-Fast gate (empty candidates)
    // that `Skip` closes.
    let has_candidates = cx
        .state
        .continuations
        .last()
        .and_then(crate::state::Continuation::pending_candidates)
        .is_some_and(|c| !c.is_empty());
    if has_candidates {
        return reaction_windows::resume_reaction_window(cx, response);
    }
    if matches!(response, InputResponse::Skip) {
        return reaction_windows::close_reaction_window(cx);
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
            // The teardown tail (forced-run-sibling re-drive / end-of-turn
            // resume) now lives in `advance`'s `PostOnResolution` arm, so it
            // fires from teardown regardless of which resume re-entered the
            // driver.
            skill_test::finish_skill_test(cx, &indices)
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
/// Routes on the **top** continuation frame — the prompt awaiting input — and
/// returns through [`drive`] (Slice C-plumbing). A window on top resolves via
/// [`resume_window`]; a mid-test reaction window closes, returns `Done`, and the
/// loop re-dispatches the now-top `SkillTest`. Rejects when nothing is outstanding.
///
/// A pure-Fast window (pushed by [`open_fast_window`], empty `pending_triggers`)
/// on top is a play *opportunity*: `InputResponse::Skip` closes it via
/// [`close_reaction_window`]. This covers the `MythosAfterDraws` window after all
/// Fast plays have been made and the player is done.
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
        // Event reaction windows + the forced run (`TimingPointWindow`) and the
        // framework player windows (`FastWindow`, #433) resolve through the one
        // window driver — it reads candidates/mode through the frame-agnostic
        // accessors.
        Some(Continuation::TimingPointWindow { .. } | Continuation::FastWindow { .. }) => {
            resume_window(cx, response)
        }
        // An effect node suspended in place for a controller pick (#422): the
        // top `Continuation::Effect(Leaf)` frame *is* the prompt. Route its
        // `PickSingle` to the effect-choice resume. A non-suspending effect
        // frame is never on top here (the drive steps it before yielding).
        Some(Continuation::Effect(_)) => choice::resume_effect_choice(cx, response),
        Some(Continuation::HunterMove(_)) => hunters::resume_hunter_choice(cx, response),
        Some(Continuation::SpawnEngage(_)) => hunters::resume_spawn_engage(cx, response),
        Some(Continuation::HandSizeDiscard(_)) => phases::resume_hand_size_discard(cx, response),
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
        Some(Continuation::PlayFromHand { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (hand-play disposal is \
                     framework-internal)"
                .into(),
        },
        // A `PlayerDraw` surge-chain frame never awaits input — the `drive` loop
        // drives it, and any prompt it opens (a spawn-engagement tie) sits above
        // it. If it is somehow top, no prompt is outstanding (defensive; mirrors
        // the EncounterCard arm).
        Some(Continuation::PlayerDraw { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (the Mythos draw chain is \
                     framework-internal)"
                .into(),
        },
        Some(Continuation::SkillTest(_)) => resume_skill_test_commit(cx, response),
        // An order-pick suspension parks the `AttackLoop` frame as the top frame
        // (it *is* the prompt) — route its `PickSingle` to the order resume
        // (#143). Every other `AttackLoop` stage sits beneath a reaction window
        // (the window is the prompt) and never legitimately awaits input here, so
        // it rejects defensively (mirrors the EncounterCard arm).
        Some(Continuation::AttackLoop {
            stage: crate::state::AttackLoopStage::PickOrder,
            ..
        }) => combat::resume_attack_order_pick(cx, response),
        Some(Continuation::AttackLoop { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a parked attack loop is top)"
                .into(),
        },
        // The interactive soak distribution's per-point prompt (#44/K5b): the
        // `DamageAssignment` frame is the top prompt, resumed by its `PickSingle`.
        Some(Continuation::DamageAssignment { .. }) => {
            combat::resume_damage_assignment(cx, response)
        }
        // A mid-action ActionResolution frame never awaits input — it is only
        // momentarily top inside `drive`. A ResolveInput here is spurious.
        Some(Continuation::ActionResolution { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a mid-action resolution \
                     frame is top)"
                .into(),
        },
        // Open-turn OptionId dispatch (slice 2b, #447): `ResolveInput(PickSingle(OptionId))`
        // at the open turn re-enumerates `legal_actions`, indexes by the submitted
        // `OptionId`, and forwards to `dispatch_turn_action`. The `ending: false`
        // arm is the live open turn; `ending: true` is only ever top momentarily
        // inside `drive`'s resume tail and never legitimately awaits input here.
        Some(Continuation::InvestigatorTurn { ending: false, .. }) => {
            let crate::action::InputResponse::PickSingle(opt) = response else {
                return EngineOutcome::Rejected {
                    reason: "ResolveInput: the open turn expects PickSingle(OptionId)".into(),
                };
            };
            let actions = crate::engine::enumerate::legal_actions(cx.state);
            let Some(action) = actions.get(opt.0 as usize).cloned() else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: open-turn OptionId({}) out of range (0..{})",
                        opt.0,
                        actions.len()
                    )
                    .into(),
                };
            };
            dispatch_turn_action(cx, &action)
        }
        Some(Continuation::InvestigatorTurn { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (transient rotation frame)"
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
        // The `when/at/after` coordinator frames (#434) never await input — they
        // push a child (a `TimingPoint`, a `TimingPointWindow`, a forced run)
        // that is the prompt, and the loop drives them otherwise. If one is
        // somehow top at ResolveInput, no prompt is outstanding (defensive).
        Some(Continuation::EmitEvent { .. } | Continuation::TimingPoint { .. }) => {
            EngineOutcome::Rejected {
                reason: "ResolveInput: no input prompt is outstanding (an EmitEvent/TimingPoint \
                         coordinator frame is top)"
                    .into(),
            }
        }
        None => EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        },
    };
    // An encounter-card Revelation that suspended parks its `EncounterCard`
    // frame beneath the suspension (#380); once that sub-resolution completes
    // the frame is top again and the `drive` loop's `EncounterCard` arm disposes
    // of it — discarding a treachery or spawning an enemy — and continues any
    // Mythos chain (#423). `apply_player_action` runs `drive(cx, outcome)` after
    // this returns.
    outcome
}
