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
// pub(super): the #482 resumable act/agenda-advance sub-process; driven by the
// `drive` loop and resumed via `resolve_input`.
pub(super) mod advance_reverse;
// pub(super): engine/mod.rs re-exports `suspend_for_native_choice` (pub) for
// the `cards` crate's native-leaf picks (Crypt Chill 01167, Axis A #334).
pub(super) mod choice;
// pub(super): evaluator reaches grant_resources via the full path
// crate::engine::dispatch::cards::grant_resources (a sibling of dispatch).
pub(super) mod cards;
// pub(super): the unified trigger-dispatch chokepoint (Axis-B T5a); engine/mod.rs
// re-exports queue_event + TimingEvent via pub(crate) for the GameEnd site.
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
// pub(super): engine/mod.rs re-exports ForcedTriggerPoint + queue_forced_triggers
// via pub(crate) for test_support::fire_forced_at (Task 2 of #215).
pub(super) mod forced_triggers;
pub(crate) mod hunters;
pub(super) mod phases;
// `pub(crate)` so the evaluator can reach `open_queued_reaction_window`; other
// items stay `pub(super)`-to-dispatch.
pub(crate) mod reaction_windows;
pub(crate) mod reveal;
// pub(super): engine::evaluator reaches start_skill_test for Effect::SkillTest.
pub(super) mod skill_test;
// pub(super): slot-capacity table + deficit math for asset slot enforcement (#498).
pub(super) mod slots;
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
            source,
            ability_index,
        } => abilities::activate_ability(cx, *investigator, *source, *ability_index),
        TurnAction::AdvanceAct { investigator } => {
            act_agenda::advance_act_action(cx, *investigator)
        }
    }
}

/// Build the open-turn action menu (2b, #447): the legal-action enumeration
/// surfaced as a structured [`InputRequest`](crate::engine::InputRequest).
/// `OptionId(i)` indexes [`legal_actions`](crate::engine::enumerate::legal_actions);
/// the `InvestigatorTurn` arm of [`resolve_input`] re-enumerates and dispatches.
fn turn_menu(state: &crate::state::GameState) -> crate::engine::InputRequest {
    let options = crate::engine::enumerate::legal_actions(state)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            crate::engine::ChoiceOption::new(
                crate::engine::OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(state),
            )
            .maybe_at(a.target(state))
        })
        .collect();
    let request = crate::engine::InputRequest::pick_single("Choose an action", options);
    // The prompt itself is anchored to the acting investigator's turn control, so
    // a host can suppress its "Choose an action" text structurally rather than by
    // matching the string (ADR 0011).
    match state.continuations.last() {
        Some(crate::state::Continuation::InvestigatorTurn { investigator, .. }) => {
            request.at(crate::engine::OptionTarget::TurnControl(*investigator))
        }
        _ => request,
    }
}

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// After #447/#459 the wire surface is a single variant:
/// [`ResolveInput`](PlayerAction::ResolveInput) — the open-turn action menu and
/// every framework suspension (mulligan, encounter draw, skill-test commit,
/// reaction/Fast windows, choices, soak distribution) all round-trip through
/// this one channel. Session setup is the non-logged `seat_and_open` entry
/// point; it never crosses this function.
///
/// The former pending-prompt gate (`!matches!(action, ResolveInput{..})`) was
/// removed in #459: with only `ResolveInput` existing the condition was always
/// false (dead code). Pending-prompt protection is now structural —
/// `resolve_input` validates the response against the live frame.
///
/// That structure also carries a rule rather than merely an action model: a
/// *player* acts only where the game offers a window, and with `ResolveInput`
/// the only wire variant, an interjection with no live frame to answer has no
/// representation at all. The official FAQ is consistent with the shape — asked
/// about a Fast ability used mid-resolution of another, it answers *"As long as
/// it is during a [fast] player window, yes."*
/// (`data/official-faq/Frequently_Asked_Questions.md`), conditioning the
/// permission on a window rather than granting it by the moment. A **Forced**
/// ability is the exception the same FAQ states and the forced path implements:
/// it needs no window and nests where it fires.
pub fn apply_player_action(cx: &mut Cx, action: &PlayerAction) -> EngineOutcome {
    let outcome = match action {
        PlayerAction::ResolveInput { response } => resolve_input(cx, response),
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
/// advance the top continuation frame until the engine blocks or idles.
/// [`drive_frames`] is the loop proper; this wrapper owns the one rule that
/// applies to *every* suspension the engine can surface, wherever it came from.
///
/// A `Rejected` outcome, and any suspension, passes straight through — unless
/// it is the prompt of a frame the scenario's ending has cancelled (#566), in
/// which case the prompt is dropped and driving resumes. That covers both a
/// suspension the action itself raised (Mythos step 1.3's doom crossing the
/// terminal agenda's threshold before step 1.4 parks on the encounter-draw
/// prompt) and one raised by a step *inside* the loop that surfaced its own
/// prompt rather than returning to the loop head (a mid-chain spawn-engagement
/// tie). Asking the player to draw an encounter card, or to choose what a
/// newly-spawned enemy engages, for a scenario that has already ended is the
/// defect either way, so both go through the same predicate.
///
/// Terminates: each re-entry discards at least the cancelled top frame, and the
/// [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd) frame at the bottom
/// of the stack is never cancelled.
pub(crate) fn drive(cx: &mut Cx, mut outcome: EngineOutcome) -> EngineOutcome {
    loop {
        if matches!(outcome, EngineOutcome::Rejected { .. }) {
            return outcome;
        }
        if !matches!(outcome, EngineOutcome::Done) && !scenario_end_cancels_top(cx.state) {
            return outcome;
        }
        outcome = drive_frames(cx);
        if matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }
    }
}

/// One pass of the main loop: advance the top continuation frame until the
/// engine blocks or idles. Always entered with the equivalent of `Done` —
/// [`drive`] owns the suspension rule — and returns:
///
/// - a `*Phase` anchor on top is advanced via
///   [`phases::anchor_on_child_pop`], which runs its resume-keyed chunk and,
///   at a phase boundary, transitions by popping itself + pushing the next
///   phase's anchor (`Entry`) — the loop then advances that;
/// - an [`ActionResolution`](crate::state::Continuation::ActionResolution) frame
///   on top is resumed via [`resume_action_resolution`], which runs the
///   action's primary effect (or suppresses it if the actor was defeated);
/// - the pass stops with `AwaitingInput` when an advance suspends, and with
///   `Done` when an [`InvestigatorTurn`](crate::state::Continuation::InvestigatorTurn)
///   frame is on top (the open turn — slice 2a-i, #393), at terminal (empty
///   stack), or when an advance makes no progress (a parked phase, e.g.
///   Investigation with no active investigator).
// A single exhaustive dispatch over every steppable `Continuation` variant;
// splitting it would only scatter the one place that says what each frame does.
#[allow(clippy::too_many_lines)]
fn drive_frames(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{Continuation, ScenarioEndStep};
    loop {
        // A latched resolution cancels opportunities, not resolutions (ADR
        // 0004). Applied at the loop head rather than by sweeping the stack
        // once, so a frame queued *during* the wind-down — a reaction window
        // opened by the skill-test teardown that is still completing — is
        // classified when it reaches the top, like every other frame.
        while scenario_end_cancels_top(cx.state) {
            cx.state.continuations.pop();
        }
        #[cfg(debug_assertions)]
        assert_no_queued_ability_beneath_anchor(cx.state);
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
            // A framework Fast window on top (empty reaction candidates, so it
            // failed the guarded arm above): surface its eligible fast plays as a
            // skippable choice, or close it when none remain (#476). Re-examined
            // after each fast play resolves — the re-open loop — until the player
            // Skips or runs out of plays.
            Some(Continuation::FastWindow { .. }) => {
                match reaction_windows::drive_fast_window(cx) {
                    EngineOutcome::Done => {} // closed (no eligible plays); loop on
                    other => return other,    // the skippable prompt, or a continuation prompt
                }
            }
            // A skill test re-exposed on top (a mid-test window/effect closed):
            // step its driver. By the invariant it is top — no `rposition` /
            // `win_idx > st` self-location.
            // An act/agenda advance sub-process on top (#482): drive its step
            // machine (acknowledge → reverse → finalize). A reverse it fires
            // lands above this frame and the loop drives it first; the frame is
            // re-exposed at Finalize when the reverse pops.
            Some(Continuation::AdvanceReverse { .. }) => match advance_reverse::drive(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // #466: a one-option forced-effect acknowledge always suspends; on
            // resume it pops and the effect frame beneath resolves.
            Some(Continuation::AcknowledgeForced { .. }) => {
                return forced_triggers::drive_acknowledge_forced(cx)
            }
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
            // resolved: place the card it holds (event → discard; asset → enter
            // play, emit EnteredPlay) and pop (Slice D #423). Never suspends
            // itself — any reaction window queued by queue_event lands on top and
            // the loop drives it next.
            Some(Continuation::PlayFromHand { .. }) => match cards::dispose_play_from_hand(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // The entered-location half of a Move, re-exposed once the left
            // location's queued `LeftLocation` forced abilities resolved (#569):
            // auto-engage at the destination and emit `EnteredLocation`.
            Some(Continuation::MoveEnter { .. }) => match actions::resume_move_enter(cx) {
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
            // `AwaitingInput` (a window / forced run opened). Every condition
            // walks them (#702), and a coordinator-owned one also resolves its
            // own impact mid-walk (#701/#703).
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
            // A deal of damage, mid-procedure (#727): step its cursor —
            // distribute, announce the assignment, place it, resume the caller.
            // Each emit lands a coordinator above this frame and the loop drives
            // that first; the frame is re-exposed when the coordinator pops.
            Some(Continuation::DealDamage { .. }) => match combat::drive_deal_damage(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
            // A parked enemy-attack loop re-exposed once the head attacker's
            // `EnemyAttacks` coordinator popped (#704): take the head off,
            // exhaust it (enemy phase), and either begin the next attack, prompt
            // for the order, or run the loop's source-keyed tail. The
            // `PickOrder` stage is a prompt the player owes an answer to, so it
            // idles in the `_` arm instead.
            Some(Continuation::AttackLoop {
                stage: crate::state::AttackLoopStage::Attacking,
                ..
            }) => match combat::drive_parked_attack_loop(cx) {
                EngineOutcome::Done => {}
                other => return other,
            },
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
            // The open turn surfaces its legal-action enumeration as an
            // `AwaitingInput` menu (2b, #447): gameplay input is now solely
            // `ResolveInput(PickSingle(OptionId))` against this frame. The menu
            // is re-enumerated at resolve (not cached) — see the
            // `InvestigatorTurn { ending: false }` arm of `resolve_input`.
            Some(Continuation::InvestigatorTurn { ending: false, .. }) => {
                return EngineOutcome::AwaitingInput {
                    request: turn_menu(cx.state),
                    // Deterministic resume-token is #458; placeholder like every
                    // other `AwaitingInput` site until then.
                    resume_token: crate::engine::ResumeToken(0),
                };
            }
            // An investigator's elimination, mid-sequence (#638). Step 0's
            // weakness-scoped game-end emit is in tail position (it only queues
            // — ADR 0003), so Cover Up 01007's trauma resolves above this frame;
            // the loop re-exposes it and steps 1–6 run, removing those same
            // weaknesses from the game. See `Continuation::Elimination`.
            Some(Continuation::Elimination { .. }) => match elimination::drive_elimination(cx) {
                EngineOutcome::Done => {} // emitted / steps ran + popped; loop on
                other => return other,    // 2+ simultaneous: the lead orders them
            },
            // The scenario's ending, exposed once everything above it has
            // completed or been cancelled (#566). `EmitGameEnd` advances the
            // cursor *before* emitting — a tail-position emit, since the emit
            // only queues (ADR 0003) and Cover Up 01007's trauma (plus its
            // interactive acknowledge) must resolve above this frame, possibly
            // across an `apply` boundary. At `Finalize` the loop stops and hands
            // the frame to the apply boundary, which holds the `ScenarioRegistry`.
            Some(Continuation::ScenarioEnd {
                step: ScenarioEndStep::EmitGameEnd,
            }) => {
                let Some(Continuation::ScenarioEnd { step }) = cx.state.continuations.last_mut()
                else {
                    unreachable!("drive: the ScenarioEnd arm ran without one on top");
                };
                *step = ScenarioEndStep::Finalize;
                match emit::queue_event(cx, &emit::TimingEvent::GameEnd) {
                    EngineOutcome::Done => {} // queued; loop on to drain it
                    other => return other,    // 2+ simultaneous: the lead orders them
                }
            }
            // Idle: an empty `FastWindow` permissive gate, terminal (empty), a
            // suspension on top (which a handler already surfaced as
            // AwaitingInput), or a `ScenarioEnd` frame at `Finalize` — the loop
            // has nothing left to drive there, and `apply` finalizes it.
            _ => return EngineOutcome::Done,
        }
    }
}

/// Whether the top frame is one the scenario's ending cancels (#566) — so
/// [`drive`] discards its prompt rather than surfacing it, and
/// [`drive_frames`] pops it rather than dispatching it. Only ever true once a
/// resolution has latched; the
/// [`ScenarioEnd`](crate::state::Continuation::ScenarioEnd) frame sits at the
/// bottom of the stack from that moment, so this cannot drain the stack.
fn scenario_end_cancels_top(state: &crate::state::GameState) -> bool {
    state.ending.is_some()
        && state
            .continuations
            .last()
            .is_some_and(crate::state::Continuation::cancelled_by_scenario_end)
}

/// Backstop for the ADR-0003 defect class (#569): a queued ability frame must
/// never be buried beneath a phase anchor.
///
/// Phase anchors **pop-and-push** rather than drain — a transition pops the
/// outgoing anchor and pushes the incoming one on whatever is left — so an
/// ability frame that ends up below one is not merely mis-ordered, it is
/// stranded at the bottom of the stack for the rest of the scenario. That is
/// exactly how agenda 01107's Ghoul movement was lost: `enemy_phase_end` read
/// the emit's `Done` as "nothing happened" and pushed the Upkeep anchor over the
/// frame it had just queued.
///
/// Checked at every `drive` step with an anchor on top (debug builds only), so
/// it fires at the transition that made the mistake rather than several phases
/// later when the ability visibly fails to have happened. What counts as queued
/// is [`Continuation::is_queued_ability`], beside the anchor predicate it pairs
/// with.
#[cfg(debug_assertions)]
fn assert_no_queued_ability_beneath_anchor(state: &crate::state::GameState) {
    let Some((top, beneath)) = state.continuations.split_last() else {
        return;
    };
    if !top.is_phase_anchor() {
        return;
    }
    let buried = beneath
        .iter()
        .position(crate::state::Continuation::is_queued_ability);
    assert!(
        buried.is_none(),
        "a queued ability frame is buried beneath the {top:?} anchor at depth {depth} \
         ({frame:?}) — a phase anchor was pushed over an ability a timing-point emit had \
         queued, which strands it (#569). Emit in tail position and resume via a frame; \
         see docs/adr/0003-emitting-a-timing-point-queues-abilities.md.",
        depth = buried.unwrap_or_default(),
        frame = buried.map(|i| &beneath[i]),
    );
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
        // A defeated actor suppresses the primary effect — but a card riding
        // this frame mid-play must still be placed, or popping the frame would
        // drop it out of the game silently, which is precisely the #604 failure.
        // In practice the frame is already empty: the only thing that flips an
        // investigator off `Active` is `apply_investigator_elimination`, whose
        // elimination steps sweep the in-progress play off this very frame.
        // Since #638 those steps may run from a `Continuation::Elimination`
        // frame rather than inline, so what guarantees "already empty" here is
        // frame *ordering* rather than synchrony: the elimination frame is
        // pushed above this one and therefore drains first. This
        // arm is the same rule applied at the same moment for any future
        // suppression cause that isn't elimination — RR p.10 step 1, an
        // eliminated investigator's owned cards are removed from the game — so
        // the card lands in the one pile elimination does not drain, exactly
        // once. See `docs/adr/0002-in-progress-play-lives-on-its-frame.md`.
        if let ActionResume::PlayCard { card: Some(card) } = resume {
            if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
                inv.removed_from_game.push(card);
            }
        }
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
            source,
            designator,
            effect,
        } => abilities::resume_activate_ability(
            cx,
            investigator,
            source,
            designator.as_ref(),
            &effect,
        ),
        ActionResume::PlayCard { card } => {
            let Some(card) = card else {
                unreachable!(
                    "resume_action_resolution: the play frame for {investigator:?} lost its \
                     card while they are still Active — elimination is the only thing that \
                     empties an ActionResolution frame (see \
                     Continuation::take_play_in_progress), and it flips status first"
                );
            };
            cards::resume_play_card(cx, investigator, card)
        }
    }
}

/// Seat a roster and drive to the first `AwaitingInput` (the setup mulligan),
/// without going through a logged `PlayerAction`. The engine entry point
/// [`crate::seat_and_open`] wraps this in the shared `apply_via` scaffolding.
/// Used at game creation (server `GameSession::create`); the action log that
/// follows is `ResolveInput`-only.
pub(crate) fn seat_and_open(cx: &mut Cx, roster: &[crate::action::RosterEntry]) -> EngineOutcome {
    let outcome = phases::start_scenario(cx, roster);
    drive(cx, outcome)
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
/// `is_fast` is consumed by [`any_fast_play_eligible`]; `abilities` is kept for
/// future consumers (e.g. reaction-window dispatch).
///
/// The card's destination is deliberately **not** here: commencing a play is
/// destination-agnostic (asset and event alike leave hand at RR Appendix I step
/// 3), and the disposal that needs it re-derives it from the code at step 4
/// (#604).
///
/// `#[allow(dead_code)]` covers `abilities` (not yet read outside validation)
/// and suppresses the rustc `dead_code` lint on struct fields that are only read
/// by a `pub(super)` function not yet wired up.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct PlayCheckResult {
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
    /// The card code of the source card.
    pub source_code: CardCode,
    /// Action points this activation costs: the ability's
    /// `Trigger::Activated` cost **plus** any `ExtraActionCost` surcharge on
    /// the action class its designator names (#754). What the affordability
    /// check compares and what payment spends — the printed cost alone is not
    /// what the investigator pays.
    pub action_cost: u8,
    /// The `first_each_round` surcharge sources that `action_cost` charged
    /// for, to mark spent once the activation commits. Empty unless the
    /// surcharge applied. Kept beside the cost so the peek stays read-only
    /// for validate-first.
    pub surcharge_sources: Vec<crate::state::CardInstanceId>,
    /// The bold action designator the ability prints, if any — what the
    /// attack-of-opportunity exemption reads (#696) and what names the action
    /// class the surcharge keys on (#754).
    pub designator: Option<crate::dsl::ActionDesignator>,
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
    // A framework Fast window (no reaction candidates): the player either plays an
    // eligible fast card / ability (PickSingle into the re-enumerated list) or
    // passes (Skip). After a pick, dispatch the play and return — the `drive` loop
    // re-examines the still-top FastWindow and re-emits (play another) or closes
    // (#476).
    match response {
        InputResponse::Skip => reaction_windows::close_reaction_window(cx),
        InputResponse::PickSingle(opt) => {
            let i = opt.0;
            let plays = reaction_windows::enumerate_fast_plays(cx.state);
            let Some(action) = plays.get(i as usize).cloned() else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: fast-window PickSingle({i}) out of range (0..{})",
                        plays.len(),
                    )
                    .into(),
                };
            };
            dispatch_turn_action(cx, &action)
        }
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a Fast-play window is open; submit PickSingle(OptionId) to play, \
                 or Skip to pass, got {other:?}",
            )
            .into(),
        },
    }
}

/// Resume a skill test parked at one of its two suspension points: the commit
/// window (the active investigator submits their commit list via
/// [`InputResponse::PickMultiple`], each [`OptionId`](crate::engine::OptionId) a
/// hand index) or the #478 acknowledgment pause (the player dismisses the result
/// with [`InputResponse::Confirm`]). Each resume validates the cursor it expects.
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
        // The cosmetic acknowledgment pause (#478) is the SkillTest frame's other
        // suspension point; a Confirm advances past it into the ST.7 consequences.
        InputResponse::Confirm => skill_test::acknowledge_outcome(cx),
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: the skill-test window expects InputResponse::PickMultiple \
                 (commit) or InputResponse::Confirm (acknowledge), got {other:?}",
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
// One exhaustive arm per `Continuation` variant, as in `drive`: splitting it
// would scatter the single place that says which frame a response routes to.
#[allow(clippy::too_many_lines)]
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
        // A `MoveEnter` frame never awaits input (#569) — the loop drives it the
        // moment the left-location abilities above it resolve. Defensive, as for
        // `PlayFromHand`.
        Some(Continuation::MoveEnter { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (the entered-location step of \
                     a move is framework-internal)"
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
        // The advance acknowledge pause (#482/#558): the single on-card advance
        // pick (`PickSingle(0)`) resumes the AdvanceReverse frame past its AwaitAck
        // step into firing the leaving card's reverse.
        Some(Continuation::AdvanceReverse { .. }) => advance_reverse::resume(cx, response),
        // #466: the one-option forced-effect acknowledge — its PickSingle pops the
        // frame so the `drive` loop resolves the effect beneath.
        Some(Continuation::AcknowledgeForced { .. }) => {
            forced_triggers::resume_acknowledge_forced(cx, response)
        }
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
        // The interactive soak distribution's per-point prompt (#44/K5b): a
        // `DealDamage` frame at its `Distribute` step is the top prompt, resumed
        // by its `PickSingle`. Its other three steps are internal sequencing the
        // loop dispatches on sight and never await input, so they reject
        // defensively (the `AttackLoop` contract).
        Some(Continuation::DealDamage {
            step: crate::state::DealDamageStep::Distribute { .. },
            ..
        }) => combat::resume_damage_distribution(cx, response),
        Some(Continuation::DealDamage { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (a deal of damage \
                     is mid-sequence)"
                .into(),
        },
        // The interactive slot make-room choice (#498): the `SlotDiscard` frame
        // is the top prompt, resumed by its `PickSingle`.
        Some(Continuation::SlotDiscard { .. }) => slots::resume_slot_discard(cx, response),
        // The scenario's ending never awaits input (#566): the acknowledge /
        // ordering run its `GameEnd` emit queues sits above it and is the prompt,
        // and the apply boundary finalizes without asking. Defensive, as for
        // `EncounterCard`.
        Some(Continuation::ScenarioEnd { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (the scenario has ended)".into(),
        },
        // An in-progress elimination never awaits input either (#638): the
        // acknowledge / ordering run its step-0 emit queues sits above it and is
        // the prompt, and steps 1–6 ask nothing. Defensive, as for `ScenarioEnd`.
        Some(Continuation::Elimination { .. }) => EngineOutcome::Rejected {
            reason: "ResolveInput: no input prompt is outstanding (an investigator's elimination \
                     is in progress)"
                .into(),
        },
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

#[cfg(test)]
mod turn_menu_tests {
    use super::turn_menu;
    use crate::engine::enumerate::legal_actions;
    use crate::engine::OptionTarget;
    use crate::state::{Continuation, InvestigationResume, InvestigatorId, Phase};
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

    #[test]
    fn turn_menu_carries_action_targets() {
        // An open-turn state with a co-located, engaged enemy so the menu holds
        // at least one Enemy-anchored option (Fight/Evade), proving turn_menu
        // propagates each action's target — not just Global.
        let mut state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_chaos_bag(crate::state::ChaosBag::new([
                crate::state::ChaosToken::Numeric(0),
            ]))
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build();
        let loc = test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        {
            let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
            inv.current_location = Some(loc_id);
            inv.actions_remaining = 3;
        }
        let mut e = test_enemy(7, "Ghoul");
        e.engaged_with = Some(InvestigatorId(1));
        e.current_location = Some(loc_id);
        state.enemies.insert(e.id, e);

        let actions = legal_actions(&state);
        let menu = turn_menu(&state);
        assert_eq!(menu.options.len(), actions.len());
        for (i, action) in actions.iter().enumerate() {
            assert_eq!(menu.options[i].target, action.target(&state));
        }
        assert!(
            menu.options
                .iter()
                .any(|o| matches!(o.target, Some(OptionTarget::Enemy(_)))),
            "expected at least one Enemy-anchored option, got {:?}",
            menu.options
                .iter()
                .map(|o| o.target.clone())
                .collect::<Vec<_>>()
        );
        // The prompt itself is anchored, so the banner can suppress its
        // "Choose an action" text structurally rather than by matching the
        // string (ADR 0011, #541).
        assert_eq!(
            menu.target,
            Some(OptionTarget::TurnControl(InvestigatorId(1))),
            "the open-turn menu anchors to the acting investigator's turn control"
        );
    }
}
