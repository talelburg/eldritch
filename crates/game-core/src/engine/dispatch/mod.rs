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
// pub(super): evaluator reaches grant_resources via the full path
// crate::engine::dispatch::cards::grant_resources (a sibling of dispatch).
pub(super) mod cards;
mod combat;
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
mod hunters;
mod phases;
mod reaction_windows;
pub(crate) mod reveal;
// pub(super): engine::evaluator reaches start_skill_test for Effect::SkillTest.
pub(super) mod skill_test;
mod threat_area;

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// Phase-1 minimal coverage: [`StartScenario`](PlayerAction::StartScenario)
/// and [`EndTurn`](PlayerAction::EndTurn) are implemented end-to-end;
/// other variants return [`EngineOutcome::Rejected`] with a TODO message
/// so callers and tests get a useful signal rather than a silent no-op.
#[allow(clippy::too_many_lines)] // dispatcher: a guard ladder + one match arm per PlayerAction
pub fn apply_player_action(cx: &mut Cx, action: &PlayerAction) -> EngineOutcome {
    // While a mulligan is pending (the setup mulligan cursor is `Some`),
    // only Mulligan (and the already-rejected re-StartScenario) is valid.
    // Per the Rules Reference, "after all players have completed their
    // mulligans, the game begins" — the engine enforces that by gating
    // other actions until every investigator has signaled their mulligan
    // choice.
    if cx.state.mulligan_pending.is_some()
        && !matches!(
            action,
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario { .. }
        )
    {
        return EngineOutcome::Rejected {
            reason: "a setup mulligan is pending; investigators must submit \
                     PlayerAction::Mulligan (with an empty indices_to_redraw to \
                     keep their hand) in player order before any other action"
                .into(),
        };
    }

    // Reaction-window guard runs BEFORE the skill-test guard: when a
    // window opens mid-skill-test (e.g. Roland's "after you defeat an
    // enemy" firing during a Fight that defeats), both
    // `in_flight_skill_test` and the open reaction window on
    // `cx.state.open_windows` are populated — the test is mid-resolution,
    // parked at the window boundary inside `drive_skill_test`. The
    // reaction-window message is the one the client needs.
    if cx.state.top_reaction_window().is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a reaction window is open; submit a \
                     PlayerAction::ResolveInput with an InputResponse::PickIndex \
                     to fire a pending trigger, or InputResponse::Skip to close \
                     the window (rejected if forced triggers remain) before any \
                     other action"
                .into(),
        };
    }

    // While a skill test is paused at its commit window (no reaction
    // window open yet), only `ResolveInput` can advance the engine.
    // Mirrors the `mulligan_pending` guard above.
    if cx.state.in_flight_skill_test.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a skill test is paused at its commit window; submit a \
                     PlayerAction::ResolveInput with an InputResponse::CommitCards \
                     (empty indices commits no cards) before any other action"
                .into(),
        };
    }

    // Hunter movement is Enemy-phase only; it can't coexist with an open
    // reaction window or an in-flight skill test, so order among the guards
    // is immaterial — but a pending hunter choice still blocks other actions.
    if cx.state.hunter_move_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a hunter-movement choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::PickLocation (movement) or \
                     InputResponse::PickInvestigator (engagement) before any other action"
                .into(),
        };
    }

    // A pending engagement-on-spawn choice (#128) likewise blocks every
    // action but `ResolveInput`. Mirrors the hunter guard above; the two
    // never coexist (different phases), so guard order is immaterial.
    if cx.state.spawn_engage_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "an engagement-on-spawn choice is pending; submit a \
                     PlayerAction::ResolveInput with InputResponse::PickInvestigator \
                     before any other action"
                .into(),
        };
    }

    // A pending upkeep hand-size discard (#111) blocks every action but
    // `ResolveInput`. Upkeep-phase only; never coexists with the other
    // suspension modes, so guard order is immaterial.
    if cx.state.hand_size_discard_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a hand-size discard choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::DiscardCards before any other action"
                .into(),
        };
    }

    // A pending act round-end advance (#275) blocks every action but
    // `ResolveInput`. Upkeep-phase only; never coexists with the others.
    if cx.state.act_round_end_pending.is_some()
        && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason:
                "an act round-end advance choice is pending; submit a PlayerAction::ResolveInput \
                     with InputResponse::Confirm or Skip before any other action"
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
        PlayerAction::Move {
            investigator,
            destination,
        } => actions::move_action(cx, *investigator, *destination),
        PlayerAction::Draw { investigator } => cards::draw(cx, *investigator),
        PlayerAction::Mulligan {
            investigator,
            indices_to_redraw,
        } => cards::mulligan(cx, *investigator, indices_to_redraw),
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
        PlayerAction::DrawEncounterCard => match cx.state.mythos_draw_pending {
            // DrawEncounterCard carries no investigator payload — the
            // acting investigator IS the pending cursor.
            Some(actor) => encounter::draw_encounter_card(cx, actor),
            None => EngineOutcome::Rejected {
                reason: "DrawEncounterCard: no draw pending (all investigators have drawn)".into(),
            },
        },
        PlayerAction::ResolveInput { response } => resolve_input(cx, response),
        PlayerAction::AdvanceAct { investigator } => {
            act_agenda::advance_act_action(cx, *investigator)
        }
    };

    // After a successful Mulligan, check whether every investigator
    // has now mulliganed. If so, the cursor reaches `None` and normal
    // play begins. Assumes `mulligan()` only ever returns `Done` or
    // `Rejected` (never `AwaitingInput`) — if it ever grows an
    // input-prompt path, this gate must be revisited so the cursor
    // doesn't silently stay set across a partial mulligan.
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && cx.state.mulligan_pending.is_none()
    {
        // Setup complete — "the game begins" (Rules Reference p.27).
        // Round 1 skips the Mythos phase (p.24), so the first phase to
        // begin is Investigation. Kick off its driver HERE, not in
        // start_scenario: setup has "no action windows" (p.27), so the
        // post-2.1 player window must not open until mulligans are done.
        //
        // NOTE: investigation_phase may leave an InvestigationBegins
        // window open (when a Fast-eligible play exists); this function
        // still returns the Mulligan's `Done`. So this is one of the few
        // paths where `Done` can accompany a non-empty `cx.state.open_windows`
        // — hosts check `open_windows` and present `ResolveInput::Skip`
        // to close it, exactly as for the phase-transition windows the
        // void `*_phase` drivers open.
        phases::investigation_phase(cx);
    }

    // Reaction windows open at the step boundary inside the handler
    // that queued them (see `drive_skill_test`), not at this outer
    // boundary — the Rules Reference clause "after… may be used
    // immediately after that triggering condition's impact upon the
    // game state has resolved" is mid-action, not post-action. Any
    // future action that queues a window outside the skill-test
    // driver must add its own boundary check; there's no fallback
    // here.

    outcome
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
    // Hunter movement, spawn engagement, and hand-size discard are three
    // mutually exclusive suspension modes (different phases). Route to the
    // right resume handler before the reaction-window and skill-test checks.
    debug_assert!(
        [
            cx.state.hunter_move_pending.is_some(),
            cx.state.spawn_engage_pending.is_some(),
            cx.state.hand_size_discard_pending.is_some(),
            cx.state.act_round_end_pending.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count()
            <= 1,
        "hunter movement, spawn engagement, hand-size discard, and act round-end advance are \
         mutually exclusive suspension modes (different phases)",
    );
    if cx.state.hunter_move_pending.is_some() {
        return hunters::resume_hunter_choice(cx, response);
    }

    // Engagement-on-spawn suspension (#128, option A) is a distinct mode
    // from hunter movement: its resume re-enters the Mythos draw chain.
    if cx.state.spawn_engage_pending.is_some() {
        return hunters::resume_spawn_engage(cx, response);
    }

    // Upkeep step-4.5 hand-size discard (#111) is its own suspension mode;
    // route it before the reaction-window check (it arises in Upkeep, not
    // mid-skill-test, so the two never coexist).
    if cx.state.hand_size_discard_pending.is_some() {
        return phases::resume_hand_size_discard(cx, response);
    }

    // Act round-end clue-spend window (#275): its own suspension mode,
    // arising only in Upkeep (never mid-skill-test), so route it before the
    // reaction-window check like hand-size discard.
    if cx.state.act_round_end_pending.is_some() {
        return phases::resume_act_round_end_advance(cx, response);
    }

    if cx.state.top_reaction_window().is_some() {
        return reaction_windows::resume_reaction_window(cx, response);
    }

    // Pure-Fast window path (Option B): no reaction-driven window is
    // pending, but a window (e.g. MythosAfterDraws) may still be on the
    // stack with empty pending_triggers. Skip is the only valid response
    // here — PickIndex / CommitCards reject below.
    if !cx.state.open_windows.is_empty() {
        if matches!(response, InputResponse::Skip) {
            let idx = cx.state.open_windows.len() - 1;
            return reaction_windows::close_reaction_window_at(cx, idx);
        }
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: a Fast-play window is open (no pending triggers); \
                 submit InputResponse::Skip to close it, got {response:?}",
            )
            .into(),
        };
    }

    if cx.state.in_flight_skill_test.is_none() {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        };
    }
    match response {
        InputResponse::CommitCards { indices } => skill_test::finish_skill_test(cx, indices),
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit window expects InputResponse::CommitCards, \
                 got {other:?}",
            )
            .into(),
        },
    }
}
