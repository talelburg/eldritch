//! Per-action dispatch handlers.
//!
//! Each function applies a single action variant to the state, mutating
//! the state in place and pushing the resulting events onto the events
//! buffer. Returns the [`EngineOutcome`] for the action.
//!
//! Handlers are split by `Action` bucket: [`apply_player_action`] for
//! human-initiated actions, [`apply_engine_record`] for engine-emitted
//! ones.

use std::collections::BTreeSet;

use crate::action::{EngineRecord, InputResponse, PlayerAction};
use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::{
    discover_clue, Cost, EventPattern, EventTiming, LocationTarget, SkillTestKind, Trigger,
};
use crate::event::{Event, FailureReason};
use crate::state::{
    resolve_token, CardCode, CardInPlay, CardInstanceId, DefeatCause, Enemy, EnemyId,
    FastActorScope, FinishContinuation, GameState, InFlightSkillTest, Investigator, InvestigatorId,
    LocationId, OpenWindow, PendingTrigger, Phase, SkillKind, SkillTestFollowUp, Status,
    TokenResolution, WindowKind, Zone,
};

use super::evaluator::{
    apply_effect, constant_skill_modifier, pending_skill_modifier, EvalContext,
};
use super::outcome::{EngineOutcome, InputRequest, ResumeToken};

/// Action points granted to an investigator at the start of their
/// turn during the Investigation phase. Per the Arkham Horror LCG
/// rulebook.
const ACTIONS_PER_TURN: u8 = 3;

/// Starting hand size at scenario setup. Per the Rules Reference,
/// each investigator draws 5 cards before mulligan.
const INITIAL_HAND_SIZE: u8 = 5;

/// Apply a [`PlayerAction`] to the state, pushing events.
///
/// Phase-1 minimal coverage: [`StartScenario`](PlayerAction::StartScenario)
/// and [`EndTurn`](PlayerAction::EndTurn) are implemented end-to-end;
/// other variants return [`EngineOutcome::Rejected`] with a TODO message
/// so callers and tests get a useful signal rather than a silent no-op.
pub fn apply_player_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    action: &PlayerAction,
) -> EngineOutcome {
    // While the mulligan window is open, only Mulligan (and the
    // already-rejected re-StartScenario) is valid. Per the Rules
    // Reference, "after all players have completed their mulligans,
    // the game begins" — the engine enforces that by gating other
    // actions until every investigator has signaled their mulligan
    // choice.
    if state.mulligan_window
        && !matches!(
            action,
            PlayerAction::Mulligan { .. } | PlayerAction::StartScenario
        )
    {
        return EngineOutcome::Rejected {
            reason: "mulligan window is still open; all investigators must submit \
                     PlayerAction::Mulligan (with an empty indices_to_redraw to \
                     keep their hand) before any other action"
                .into(),
        };
    }

    // Reaction-window guard runs BEFORE the skill-test guard: when a
    // window opens mid-skill-test (e.g. Roland's "after you defeat an
    // enemy" firing during a Fight that defeats), both
    // `in_flight_skill_test` and the open reaction window on
    // `state.open_windows` are populated — the test is mid-resolution,
    // parked at the window boundary inside `drive_skill_test`. The
    // reaction-window message is the one the client needs.
    if state.top_reaction_window().is_some() && !matches!(action, PlayerAction::ResolveInput { .. })
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
    // Mirrors the `mulligan_window` guard above.
    if state.in_flight_skill_test.is_some() && !matches!(action, PlayerAction::ResolveInput { .. })
    {
        return EngineOutcome::Rejected {
            reason: "a skill test is paused at its commit window; submit a \
                     PlayerAction::ResolveInput with an InputResponse::CommitCards \
                     (empty indices commits no cards) before any other action"
                .into(),
        };
    }

    let outcome = match action {
        PlayerAction::StartScenario => start_scenario(state, events),
        PlayerAction::EndTurn => end_turn(state, events),
        PlayerAction::PerformSkillTest {
            investigator,
            skill,
            difficulty,
        } => perform_skill_test(state, events, *investigator, *skill, *difficulty),
        PlayerAction::Investigate { investigator } => investigate(state, events, *investigator),
        PlayerAction::Move {
            investigator,
            destination,
        } => move_action(state, events, *investigator, *destination),
        PlayerAction::Draw { investigator } => draw(state, events, *investigator),
        PlayerAction::Mulligan {
            investigator,
            indices_to_redraw,
        } => mulligan(state, events, *investigator, indices_to_redraw),
        PlayerAction::Fight {
            investigator,
            enemy,
        } => fight(state, events, *investigator, *enemy),
        PlayerAction::Evade {
            investigator,
            enemy,
        } => evade(state, events, *investigator, *enemy),
        PlayerAction::PlayCard {
            investigator,
            hand_index,
        } => play_card(state, events, *investigator, *hand_index),
        PlayerAction::ActivateAbility {
            investigator,
            instance_id,
            ability_index,
        } => activate_ability(state, events, *investigator, *instance_id, *ability_index),
        PlayerAction::ResolveInput { response } => resolve_input(state, events, response),
    };

    // After a successful Mulligan, check whether every investigator
    // has now mulliganed. If so, the setup window closes and normal
    // play begins. Assumes `mulligan()` only ever returns `Done` or
    // `Rejected` (never `AwaitingInput`) — if it ever grows an
    // input-prompt path, this gate must be revisited so the window
    // doesn't silently stay open across a partial mulligan.
    if matches!(outcome, EngineOutcome::Done)
        && matches!(action, PlayerAction::Mulligan { .. })
        && state.investigators.values().all(|inv| inv.mulligan_used)
    {
        state.mulligan_window = false;
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
pub fn apply_engine_record(
    state: &mut GameState,
    events: &mut Vec<Event>,
    record: &EngineRecord,
) -> EngineOutcome {
    match record {
        EngineRecord::DeckShuffled { investigator } => deck_shuffled(state, events, *investigator),
        EngineRecord::EncounterDeckShuffled => encounter_deck_shuffled(state, events),
    }
}

/// Handler for [`EngineRecord::DeckShuffled`].
///
/// Permutes the named investigator's player deck via the deterministic
/// RNG and emits [`Event::DeckShuffled`]. Empty decks are a silent
/// no-op (no event emitted) — there's nothing to shuffle.
fn deck_shuffled(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if !state.investigators.contains_key(&investigator) {
        return EngineOutcome::Rejected {
            reason: format!("DeckShuffled: investigator {investigator:?} is not in state").into(),
        };
    }
    shuffle_player_deck(state, events, investigator);
    EngineOutcome::Done
}

/// Handler for [`EngineRecord::EncounterDeckShuffled`].
///
/// Permutes the shared encounter deck via the deterministic RNG and
/// emits [`Event::EncounterDeckShuffled`] (when ≥ 2 cards). No
/// validation — the encounter deck is shared, so there's no
/// per-investigator existence check.
fn encounter_deck_shuffled(
    state: &mut GameState,
    events: &mut Vec<Event>,
) -> EngineOutcome {
    shuffle_encounter_deck(state, events);
    EngineOutcome::Done
}

/// Fisher-Yates shuffle of the named investigator's deck using the
/// shared deterministic RNG. Used by [`deck_shuffled`] and by
/// scenario setup (initial-hand draw).
///
/// Emits [`Event::DeckShuffled`] iff the deck had at least 2 cards
/// (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_player_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
            "shuffle_player_deck: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
        });
    if inv.deck.len() < 2 {
        return;
    }
    // Fisher-Yates: walk from the end, swap each element with one in
    // [0, i]. `next_index(n)` returns `[0, n)`, so we pass i+1.
    let deck_len = inv.deck.len();
    // Collect swap indices first, then apply — avoids holding a
    // mutable borrow on `inv.deck` across the RNG calls. (next_index
    // takes &mut state.rng, which conflicts with the &mut borrow we
    // already have on the investigator if we did this inline.)
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    let inv = state.investigators.get_mut(&investigator).expect("checked");
    for (a, b) in swaps {
        inv.deck.swap(a, b);
    }
    events.push(Event::DeckShuffled { investigator });
}

/// Fisher-Yates shuffle of the shared encounter deck using the
/// shared deterministic RNG. Used by [`encounter_deck_shuffled`] and
/// by [`reshuffle_encounter_discard`].
///
/// Emits [`Event::EncounterDeckShuffled`] iff the deck had at least
/// 2 cards (a 0- or 1-card deck has nothing to permute).
pub(super) fn shuffle_encounter_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
) {
    let deck_len = state.encounter_deck.len();
    if deck_len < 2 {
        return;
    }
    // Mirror shuffle_player_deck's "collect swaps then apply" pattern:
    // RngState::next_index borrows &mut state.rng, which would conflict
    // with a &mut borrow on state.encounter_deck inline.
    let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(deck_len - 1);
    let mut i = deck_len - 1;
    while i >= 1 {
        let j = state.rng.next_index(i + 1);
        swaps.push((i, j));
        i -= 1;
    }
    for (a, b) in swaps {
        state.encounter_deck.swap(a, b);
    }
    events.push(Event::EncounterDeckShuffled);
}

/// Drain `state.encounter_discard` into `state.encounter_deck` and
/// shuffle the resulting deck. Called by
/// [`draw_encounter_top`] when the deck runs empty.
///
/// Does NOT push an `EngineRecord::EncounterDeckShuffled` to the
/// action log — mid-handler reshuffles rely on RNG determinism for
/// replay rather than log entries, mirroring the existing
/// player-deck pattern. The `EngineRecord` variant is reserved for
/// explicit shuffle actions (future "shuffle X into the encounter
/// deck" effects).
#[allow(dead_code)]
pub(super) fn reshuffle_encounter_discard(
    state: &mut GameState,
    events: &mut Vec<Event>,
) {
    state.encounter_deck.extend(state.encounter_discard.drain(..));
    shuffle_encounter_deck(state, events);
}

/// Draw the top card of the encounter deck, transparently reshuffling
/// the discard back in if the deck is empty.
///
/// Returns `Some(code)` when a card was available (either from the
/// deck directly or after the reshuffle). Returns `None` when both
/// the deck and the discard are empty — callers decide how to
/// interpret this (#69's Mythos loop treats it as a scenario
/// condition rather than an engine error).
#[allow(dead_code)]
pub(super) fn draw_encounter_top(
    state: &mut GameState,
    events: &mut Vec<Event>,
) -> Option<CardCode> {
    if state.encounter_deck.is_empty() {
        if state.encounter_discard.is_empty() {
            return None;
        }
        reshuffle_encounter_discard(state, events);
    }
    state.encounter_deck.pop_front()
}

/// Draw up to `count` cards from the named investigator's deck top
/// into their hand. Stops early (without panic) if the deck runs out
/// — this helper is just the structural move; reshuffle / horror
/// penalty logic for an empty deck lives in [`draw`].
///
/// Emits a single [`Event::CardsDrawn`] with the actually-drawn
/// count, even if that's zero. A zero-count draw is informative for
/// consumers tracking the attempt.
pub(super) fn draw_cards(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    count: u8,
) {
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "draw_cards: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    let drawn = std::cmp::min(count as usize, inv.deck.len());
    // Cards are drawn from the deck front (top). Splice out the first
    // `drawn` cards in order and append to hand.
    let drawn_cards: Vec<_> = inv.deck.drain(..drawn).collect();
    inv.hand.extend(drawn_cards);
    // `drawn` ≤ `count: u8`, so the cast can't overflow.
    let drawn_u8 = u8::try_from(drawn).expect("drawn <= count <= u8::MAX");
    events.push(Event::CardsDrawn {
        investigator,
        count: drawn_u8,
    });
}

fn start_scenario(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    // The GameState constructor places the world in its initial shape;
    // this action is the explicit "session has begun" marker that lands
    // in the action log. Replaying it on an already-started state is a
    // bug, not a no-op — reject so callers notice rather than silently
    // double-emitting `ScenarioStarted`.
    if state.round != 0 {
        return EngineOutcome::Rejected {
            reason: "StartScenario applied to a state that is already in progress".into(),
        };
    }
    // Round 1 begins at Mythos. We emit the entry event explicitly here
    // (rather than letting `step_phase` do it) because there is no
    // "previous" phase to emit a `PhaseEnded` for.
    state.round = 1;
    state.phase = Phase::Mythos;
    events.push(Event::ScenarioStarted);
    events.push(Event::PhaseStarted {
        phase: Phase::Mythos,
    });

    // For each investigator (sorted by id for determinism), shuffle
    // their deck and deal an initial hand of up to 5.
    let inv_ids: Vec<InvestigatorId> = state.investigators.keys().copied().collect();
    for inv_id in inv_ids {
        shuffle_player_deck(state, events, inv_id);
        draw_cards(state, events, inv_id, INITIAL_HAND_SIZE);
    }

    // Open the mulligan window. Each investigator may now submit a
    // single `PlayerAction::Mulligan` to redraw a subset of their
    // starting hand. The window closes once every investigator has
    // `mulligan_used == true` (see `apply_player_action`); other
    // player actions are rejected until then.
    state.mulligan_window = true;

    // Phase 1: Mythos / Enemy / Upkeep have no content yet, so we
    // tick straight through them. Once the engine grows real Mythos
    // draws and Enemy attacks, these phases stop being free skips.
    step_phase(state, events); // Mythos → Investigation
    if let Some(&first) = state.turn_order.first() {
        rotate_to_active(state, events, first);
    }
    EngineOutcome::Done
}

fn end_turn(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: "EndTurn is only valid during the Investigation phase".into(),
        };
    }
    let Some(active_id) = state.active_investigator else {
        return EngineOutcome::Rejected {
            reason: "EndTurn requires an active investigator".into(),
        };
    };
    // The Some(active_investigator) invariant is paired with that ID
    // existing in the investigators map; a missing entry would be state
    // corruption, not a normal rejection. Surface it loudly rather than
    // hiding behind Rejected.
    let active = state.investigators.get_mut(&active_id).unwrap_or_else(|| {
        unreachable!(
            "active_investigator {active_id:?} is not in the investigators map; \
                 this is a state-corruption invariant violation"
        )
    });

    // Drain remaining actions and announce the turn ended.
    if active.actions_remaining != 0 {
        active.actions_remaining = 0;
        events.push(Event::ActionsRemainingChanged {
            investigator: active_id,
            new_count: 0,
        });
    }
    events.push(Event::TurnEnded {
        investigator: active_id,
    });

    // If there's another investigator after this one in turn order,
    // rotate. Otherwise the Investigation phase ends and we tick
    // through the rest of the round automatically (Phase 1: empty
    // Enemy/Upkeep/Mythos), arriving back at Investigation with the
    // first investigator active.
    let next = state
        .turn_order
        .iter()
        .position(|id| *id == active_id)
        .and_then(|idx| state.turn_order.get(idx + 1).copied());

    if let Some(next_id) = next {
        rotate_to_active(state, events, next_id);
    } else {
        state.active_investigator = None;
        step_phase(state, events); // Investigation → Enemy
        step_phase(state, events); // Enemy → Upkeep
        step_phase(state, events); // Upkeep → Mythos (round bumps)
        step_phase(state, events); // Mythos → Investigation
        if let Some(&first) = state.turn_order.first() {
            rotate_to_active(state, events, first);
        }
    }

    EngineOutcome::Done
}

/// Transition to the next phase: emit `PhaseEnded` for the current
/// phase, advance, emit `PhaseStarted` for the new one. Bumps the
/// round counter when entering [`Phase::Mythos`] (which is the start
/// of a new round).
///
/// **Round-bump invariant:** this is the only path that bumps
/// `state.round` post-`StartScenario`. A future caller that wants to
/// step phases for a non-round-cycle reason (e.g. a scenario effect
/// that skips a phase) will need to suppress the bump here, or the
/// round counter will drift. Revisit when such a use case appears.
fn step_phase(state: &mut GameState, events: &mut Vec<Event>) {
    let from = state.phase;
    let to = from.next();
    events.push(Event::PhaseEnded { phase: from });
    state.phase = to;
    if to == Phase::Mythos {
        state.round += 1;
    }
    events.push(Event::PhaseStarted { phase: to });
}

/// Set `active_investigator` to `id` and refresh that investigator's
/// action points to the per-turn cap (3). Emits `ActionsRemainingChanged`.
///
/// `id` must refer to an investigator in `state.investigators` —
/// callers that pass an id from `state.turn_order` are guaranteed
/// this by the whole-program invariant "every id in `turn_order`
/// exists in `investigators`." A missing entry would be state
/// corruption, not a normal error.
fn rotate_to_active(state: &mut GameState, events: &mut Vec<Event>, id: InvestigatorId) {
    state.active_investigator = Some(id);
    let inv = state.investigators.get_mut(&id).unwrap_or_else(|| {
        unreachable!(
            "rotate_to_active: investigator {id:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    inv.actions_remaining = ACTIONS_PER_TURN;
    events.push(Event::ActionsRemainingChanged {
        investigator: id,
        new_count: ACTIONS_PER_TURN,
    });
}

/// Open the commit window for a skill test.
///
/// Validates the test (investigator exists and is Active, chaos bag is
/// non-empty, difficulty non-negative, no other test already in
/// flight), pushes [`Event::SkillTestStarted`], stores an
/// [`InFlightSkillTest`] on `state`, and returns
/// [`EngineOutcome::AwaitingInput`]. The active investigator finishes
/// the test by submitting a
/// [`PlayerAction::ResolveInput`](crate::action::PlayerAction::ResolveInput)
/// carrying [`InputResponse::CommitCards`].
///
/// On validation failure, returns [`EngineOutcome::Rejected`] with no
/// state change and no events pushed.
pub(super) fn start_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
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
    let Some(inv) = state.investigators.get(&investigator) else {
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
    if state.chaos_bag.tokens.is_empty() {
        return EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        };
    }
    if difficulty < 0 {
        return EngineOutcome::Rejected {
            reason: format!("skill test: difficulty {difficulty} must be >= 0").into(),
        };
    }
    if state.in_flight_skill_test.is_some() {
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
    state.in_flight_skill_test = Some(InFlightSkillTest {
        investigator,
        skill,
        kind,
        difficulty,
        committed_by_active: Vec::new(),
        tested_location,
        follow_up,
        continuation: FinishContinuation::AwaitingCommit,
    });
    events.push(Event::SkillTestStarted {
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
fn finish_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    indices: &[u32],
) -> EngineOutcome {
    // Snapshot the in-flight record (Copy-able primitives only) so
    // later mutation paths can re-borrow state freely.
    let Some(in_flight) = state.in_flight_skill_test.as_ref() else {
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
    let indices_u8 = match validate_commit_indices(state, investigator, indices) {
        Ok(v) => v,
        Err(rejected) => return rejected,
    };

    let skill_value = sum_skill_value(state, investigator, skill, kind, &indices_u8);

    // Persist the committed indices into the in-flight record for
    // replay clarity. Safe to expect: we read `in_flight_skill_test`
    // immediately above and nothing has cleared it since.
    state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .committed_by_active
        .clone_from(&indices_u8);

    let succeeded =
        resolve_chaos_token_and_emit(state, events, investigator, skill, difficulty, skill_value);

    if succeeded {
        apply_skill_test_follow_up(state, events, investigator, follow_up);
    }

    // Step 2 is complete. Advance the continuation (carrying the
    // outcome forward) and let the driver handle the remaining
    // steps (including the possibly-queued reaction window from
    // inside the follow-up).
    state
        .in_flight_skill_test
        .as_mut()
        .expect("in_flight_skill_test was Some immediately above")
        .continuation = FinishContinuation::PostFollowUp { succeeded };

    drive_skill_test(state, events)
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
fn drive_skill_test(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    loop {
        if state.top_reaction_window().is_some() {
            return open_queued_reaction_window(state, events);
        }

        let (continuation, investigator, indices_u8) = {
            let in_flight = state.in_flight_skill_test.as_ref().unwrap_or_else(|| {
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
                fire_on_skill_test_resolution(state, events, investigator, &indices_u8, succeeded);
                state
                    .in_flight_skill_test
                    .as_mut()
                    .expect("in_flight_skill_test must persist across driver steps")
                    .continuation = FinishContinuation::PostOnResolution { succeeded };
            }
            FinishContinuation::PostOnResolution { succeeded: _ } => {
                discard_committed_cards(state, events, investigator, &indices_u8);
                events.push(Event::SkillTestEnded { investigator });
                // ModifierScope::ThisSkillTest contributions expire when
                // the test ends. Drain pending entries for *this*
                // investigator only — entries queued for other
                // investigators' future tests stay.
                state
                    .pending_skill_modifiers
                    .retain(|m| m.investigator != investigator);
                state.in_flight_skill_test = None;
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
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    skill_value: i8,
) -> bool {
    let token_idx = state.rng.next_index(state.chaos_bag.tokens.len());
    let token = state.chaos_bag.tokens[token_idx];
    let resolution = resolve_token(token, &state.token_modifiers);
    events.push(Event::ChaosTokenRevealed { token, resolution });

    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let succeeded = margin >= 0 && fail_reason.is_none();
    if succeeded {
        events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        let by = difficulty.saturating_sub(total);
        events.push(Event::SkillTestFailed {
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
fn discard_committed_cards(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    indices_u8: &[u8],
) {
    let mut sorted: Vec<u8> = indices_u8.to_vec();
    sorted.sort_by(|a, b| b.cmp(a));
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "discard_committed_cards: investigator {investigator:?} vanished after \
                 follow-up; this is a state-corruption invariant violation"
            )
        });
    for idx in sorted {
        let code = inv.hand.remove(usize::from(idx));
        inv.discard.push(code.clone());
        events.push(Event::CardDiscarded {
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
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    follow_up: SkillTestFollowUp,
) {
    match follow_up {
        SkillTestFollowUp::None => {}
        SkillTestFollowUp::Investigate => {
            let effect = discover_clue(LocationTarget::ControllerLocation, 1);
            let ctx = EvalContext::for_controller(investigator);
            // Same caveat as the pre-refactor `investigate`: the only
            // remaining rejection path inside `discover_clue` is
            // "controller is between locations", which the Investigate
            // action validates before starting the test. Empty-
            // location is a silent no-op by design. Any rejection
            // here is a state-corruption invariant violation.
            let outcome = apply_effect(state, events, &effect, ctx);
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
            damage_enemy(state, events, enemy, 1, Some(investigator));
        }
        SkillTestFollowUp::Evade { enemy } => {
            let e = state.enemies.get_mut(&enemy).unwrap_or_else(|| {
                unreachable!(
                    "Evade follow-up: enemy {enemy:?} vanished while test was in flight; \
                     this is a state-corruption invariant violation"
                )
            });
            e.engaged_with = None;
            e.exhausted = true;
            events.push(Event::EnemyDisengaged {
                enemy,
                investigator,
            });
            events.push(Event::EnemyExhausted { enemy });
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
    state: &mut GameState,
    events: &mut Vec<Event>,
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
        let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
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
            let ctx = EvalContext::for_controller(investigator);
            let result = apply_effect(state, events, &ability.effect, ctx);
            if let EngineOutcome::Rejected { reason } = result {
                unreachable!(
                    "OnSkillTestResolution: effect for card {code:?} rejected unexpectedly: \
                     {reason}"
                );
            }
        }
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
fn resolve_input(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &InputResponse,
) -> EngineOutcome {
    if state.top_reaction_window().is_some() {
        return resume_reaction_window(state, events, response);
    }
    if state.in_flight_skill_test.is_none() {
        return EngineOutcome::Rejected {
            reason: "ResolveInput: no AwaitingInput prompt is currently outstanding".into(),
        };
    }
    match response {
        InputResponse::CommitCards { indices } => finish_skill_test(state, events, indices),
        other => EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: skill-test commit window expects InputResponse::CommitCards, \
                 got {other:?}",
            )
            .into(),
        },
    }
}

/// Queue a reaction window of the given `kind` if any in-play card
/// has a matching `Trigger::OnEvent` ability. No-op when the registry
/// isn't installed or no card matches.
///
/// The window doesn't open here — the surrounding handler's driver
/// (today, [`drive_skill_test`]) checks for queued windows at its
/// next step boundary and either opens them via
/// [`open_queued_reaction_window`] or proceeds to the next step.
/// Splitting queue and open keeps the event-emitter
/// (`damage_enemy` and future callers) free of suspension logic —
/// they just signal "this impact happened, scan for reactions" and
/// the driver decides when to actually pause.
///
/// Idempotency: if a window is already queued for this apply, the new
/// `kind` overwrites it. Phase-3 actions only emit one defeating
/// event per apply (a single Fight's `damage_enemy` call), so this case
/// doesn't arise; the overwrite is a loud-on-debug placeholder
/// rather than silent stacking — multi-window queueing lands when a
/// multi-defeat effect arrives.
fn queue_reaction_window(state: &mut GameState, kind: WindowKind) {
    let pending_triggers = scan_pending_triggers(state, kind);
    if pending_triggers.is_empty() {
        return;
    }
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
        // BetweenPhases windows open for phase-transition timing; no
        // Trigger::OnEvent pattern matches a phase-transition window —
        // those windows gate Fast actions, not after-event reactions.
        (WindowKind::BetweenPhases { .. }, _) => false,
    }
}

/// Emit [`Event::WindowOpened`] for the queued window and return
/// [`AwaitingInput`]. Called by [`drive_skill_test`] at a
/// step boundary when an earlier step queued a window via
/// [`queue_reaction_window`]. Future non-skill-test handlers that
/// queue windows will call this from their own boundaries.
fn open_queued_reaction_window(state: &GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let window = state
        .top_reaction_window()
        .expect("open_queued_reaction_window: caller checked is_some");
    events.push(Event::WindowOpened { kind: window.kind });
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
fn resume_reaction_window(
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
fn close_reaction_window_at(
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

    // If a skill test was mid-resolution when this window opened,
    // hand control back to its driver to run the remaining steps.
    // `AwaitingCommit` means the test is parked at the commit
    // window (no driver state to resume); this happens when a future
    // non-skill-test action queues a window — `Done` is the right
    // terminal outcome.
    if let Some(in_flight) = state.in_flight_skill_test.as_ref() {
        if !matches!(in_flight.continuation, FinishContinuation::AwaitingCommit) {
            return drive_skill_test(state, events);
        }
    }

    EngineOutcome::Done
}

/// Public dispatch wrapper for [`PlayerAction::PerformSkillTest`].
///
/// Opens the commit window with no action-specific follow-up. The
/// after-resolution trigger window (#64) is downstream.
fn perform_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    start_skill_test(
        state,
        events,
        investigator,
        skill,
        SkillTestKind::Plain,
        difficulty,
        SkillTestFollowUp::None,
    )
}

/// Handler for [`PlayerAction::Investigate`].
///
/// Spends 1 action, runs an intellect skill test against the location's
/// shroud, and on success applies [`Effect::DiscoverClue`] to move 1
/// clue from the location to the investigator. The discover-clue
/// evaluator handles the location-empty edge case as a silent no-op,
/// so an investigation at a 0-clue location costs the action and runs
/// the test but yields nothing — consistent with the rules.
///
/// Card-derived investigate variants (Rite of Seeking's "Action:
/// Investigate using willpower instead of intellect", Working a
/// Hunch's discover-without-test) implement their own paths; this
/// handler is the bare turn-action.
///
/// [`Effect::DiscoverClue`]: crate::dsl::Effect::DiscoverClue
fn investigate(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    // Validate-first.
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
    // Active-investigator + missing-from-map is a state-corruption
    // invariant violation; panic rather than silently rejecting.
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Investigate: active_investigator {investigator:?} is not in the investigators \
             map; this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Investigate requires at least 1 action point".into(),
        };
    }
    let Some(location_id) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: {investigator:?} has no current_location to investigate")
                .into(),
        };
    };
    // A `current_location` that doesn't exist in `state.locations` is
    // a state-corruption invariant violation, not a user-facing
    // rejection — match `end_turn` and `rotate_to_active` and surface
    // it loudly.
    let location = state.locations.get(&location_id).unwrap_or_else(|| {
        unreachable!(
            "Investigate: location {location_id:?} (investigator's current_location) \
             is not in the locations map; this is a state-corruption invariant violation"
        )
    });
    // Shroud is u8 in state but skill-test difficulty is i8. Saturate
    // at i8::MAX for the absurd case; realistic shrouds are 0–6.
    let difficulty = i8::try_from(location.shroud).unwrap_or(i8::MAX);

    // Mutate-second: spend the action, fire AoO, then resolve the
    // test. Investigate is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Engage, Resign are), so each ready engaged
    // enemy attacks before the test resolves.
    spend_one_action(state, events, investigator);
    fire_attacks_of_opportunity(state, events, investigator);

    // If AoO defeated the investigator, the action's primary effect
    // (the skill test) is suppressed. The action point and AoO events
    // already fired — they stay. The action declaration was legal;
    // the investigator just can't complete it.
    let inv_after_aoo = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Investigate: investigator {investigator:?} disappeared between AoO and skill test; \
             this is a state-corruption invariant violation"
        )
    });
    if inv_after_aoo.status != Status::Active {
        return EngineOutcome::Done;
    }

    start_skill_test(
        state,
        events,
        investigator,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        difficulty,
        SkillTestFollowUp::Investigate,
    )
}

/// Handler for [`PlayerAction::Move`].
///
/// Spends 1 action, then updates `current_location` to a connected
/// destination. Move is legal while engaged with enemies: per the
/// Rules Reference, each ready engaged enemy makes an attack of
/// opportunity before the move resolves, and engaged enemies move
/// with the investigator. Both behaviors land alongside enemy state
/// in #67; this handler covers only the bare movement.
fn move_action(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    destination: LocationId,
) -> EngineOutcome {
    // Validate-first.
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Move is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Move: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
    // Active-investigator + missing-from-map is a state-corruption
    // invariant violation (active_investigator is engine-set; the
    // pairing with the map entry is an invariant), so surface loudly.
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Move: active_investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Move: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Move requires at least 1 action point".into(),
        };
    }
    let Some(from) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Move: {investigator:?} has no current_location to move from").into(),
        };
    };
    if from == destination {
        return EngineOutcome::Rejected {
            reason: format!("Move: destination {destination:?} is the current location").into(),
        };
    }
    // current_location is engine-set state, so a dangling reference is
    // an invariant violation and panics. Connection lists, by contrast,
    // are scenario-data inputs — a connection pointing at a missing
    // location is malformed input, not engine corruption, so we
    // reject. Check destination-in-state BEFORE connections so the
    // error message is informative when both fail.
    let from_loc = state.locations.get(&from).unwrap_or_else(|| {
        unreachable!(
            "Move: location {from:?} (investigator's current_location) is not in the \
             locations map; this is a state-corruption invariant violation"
        )
    });
    if !state.locations.contains_key(&destination) {
        return EngineOutcome::Rejected {
            reason: format!("Move: destination {destination:?} is not in state").into(),
        };
    }
    if !from_loc.connections.contains(&destination) {
        return EngineOutcome::Rejected {
            reason: format!("Move: {destination:?} is not connected to {from:?}").into(),
        };
    }

    // Mutate-second.
    spend_one_action(state, events, investigator);

    // Move triggers attacks of opportunity from each ready engaged
    // enemy. Per the Rules Reference, this happens BEFORE the move
    // resolves.
    fire_attacks_of_opportunity(state, events, investigator);

    // If AoO defeated the investigator, the move is cancelled. The
    // action point and AoO events stay; the investigator (and any
    // engaged enemies) don't change location.
    let inv_after_aoo = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Move: investigator {investigator:?} disappeared between AoO and move resolution; \
             this is a state-corruption invariant violation"
        )
    });
    if inv_after_aoo.status != Status::Active {
        return EngineOutcome::Done;
    }

    // Engaged enemies move with the investigator. Capture the
    // engagement set before mutating any locations, then update each
    // engaged enemy's `current_location` to the destination
    // alongside the investigator's own move.
    let engaged: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked above")
        .current_location = Some(destination);
    for enemy_id in engaged {
        if let Some(enemy) = state.enemies.get_mut(&enemy_id) {
            enemy.current_location = Some(destination);
        }
    }
    events.push(Event::InvestigatorMoved {
        investigator,
        from,
        to: destination,
    });
    EngineOutcome::Done
}

/// Validate the prefix shared by Fight and Evade: phase, active
/// investigator, action point available, enemy exists, engaged with
/// the named enemy. Returns the borrowed enemy so the caller can pick
/// which difficulty (fight / evade) and read any other fields it
/// needs.
///
/// On `Err`, returns the rejection; the caller should propagate it
/// without further state mutation. State-corruption invariants
/// (active investigator missing from map) panic via `unreachable!`.
///
/// Does NOT validate the chosen difficulty is non-negative — the
/// caller must do that after picking, because Fight and Evade each
/// only care about one of the two values, and validating both
/// upfront would reject legitimate states (an enemy with `fight: -1`
/// the investigator only ever Evades).
fn validate_engaged_action<'a>(
    state: &'a GameState,
    action_name: &'static str,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> Result<&'a Enemy, EngineOutcome> {
    if state.phase != Phase::Investigation {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name} is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        });
    }
    if state.active_investigator != Some(investigator) {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name}: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        });
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "{action_name}: active_investigator {investigator:?} is not in the investigators \
             map; this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name}: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        });
    }
    if inv.actions_remaining < 1 {
        return Err(EngineOutcome::Rejected {
            reason: format!("{action_name} requires at least 1 action point").into(),
        });
    }
    let Some(enemy) = state.enemies.get(&enemy_id) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("{action_name}: enemy {enemy_id:?} is not in state").into(),
        });
    };
    if enemy.engaged_with != Some(investigator) {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name}: {investigator:?} is not engaged with {enemy_id:?} (engaged_with = {:?})",
                enemy.engaged_with,
            )
            .into(),
        });
    }
    Ok(enemy)
}

/// Spend 1 action point from the active investigator and emit
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= 1`.
fn spend_one_action(state: &mut GameState, events: &mut Vec<Event>, investigator: InvestigatorId) {
    let inv = state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked before spend_one_action");
    let new_count = inv.actions_remaining - 1;
    inv.actions_remaining = new_count;
    events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

/// Handler for [`PlayerAction::Fight`].
///
/// Spends 1 action, runs a Combat skill test against the enemy's
/// fight value, and on success deals 1 damage. If damage reaches
/// `max_health`, the enemy is defeated and removed from play.
///
/// Damage > 1 (weapons, card buffs), after-success / after-failure
/// triggers (#64), and `AoO` from *other* engaged enemies (#78) are all
/// downstream. `AoO` does NOT fire on Fight itself per the Rules
/// Reference's `AoO`-exempt list.
fn fight(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let fight_difficulty = match validate_engaged_action(state, "Fight", investigator, enemy_id) {
        Ok(enemy) => {
            if enemy.fight < 0 {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "Fight: enemy {enemy_id:?} has negative fight value {} (malformed state)",
                        enemy.fight,
                    )
                    .into(),
                };
            }
            enemy.fight
        }
        Err(rejected) => return rejected,
    };
    spend_one_action(state, events, investigator);
    start_skill_test(
        state,
        events,
        investigator,
        SkillKind::Combat,
        SkillTestKind::Fight,
        fight_difficulty,
        SkillTestFollowUp::Fight { enemy: enemy_id },
    )
}

/// Handler for [`PlayerAction::Evade`].
///
/// Spends 1 action, runs an Agility skill test against the enemy's
/// evade value, and on success disengages and exhausts the enemy.
fn evade(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let evade_difficulty = match validate_engaged_action(state, "Evade", investigator, enemy_id) {
        Ok(enemy) => {
            if enemy.evade < 0 {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "Evade: enemy {enemy_id:?} has negative evade value {} (malformed state)",
                        enemy.evade,
                    )
                    .into(),
                };
            }
            enemy.evade
        }
        Err(rejected) => return rejected,
    };
    spend_one_action(state, events, investigator);
    start_skill_test(
        state,
        events,
        investigator,
        SkillKind::Agility,
        SkillTestKind::Evade,
        evade_difficulty,
        SkillTestFollowUp::Evade { enemy: enemy_id },
    )
}

/// Apply `amount` damage to an enemy. If the new damage reaches or
/// exceeds `max_health`, emit `EnemyDefeated` and remove the enemy
/// from `state.enemies`. `by` attributes the defeat for
/// trigger-window consumers (e.g. Roland's reaction). Used by Fight
/// today; will be reused by future damage-dealing card effects.
fn damage_enemy(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
    amount: u8,
    by: Option<InvestigatorId>,
) {
    let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "damage_enemy: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    });
    let new_damage = enemy.damage.saturating_add(amount).min(enemy.max_health);
    enemy.damage = new_damage;
    events.push(Event::EnemyDamaged {
        enemy: enemy_id,
        amount,
        new_damage,
    });
    if new_damage >= enemy.max_health {
        events.push(Event::EnemyDefeated {
            enemy: enemy_id,
            by,
        });
        state.enemies.remove(&enemy_id);
        // Queue the post-defeat reaction window. The skill-test
        // driver opens it at the next step boundary (between
        // `apply_skill_test_follow_up` and `fire_on_skill_test_resolution`);
        // see `drive_skill_test`.
        queue_reaction_window(
            state,
            WindowKind::AfterEnemyDefeated {
                enemy: enemy_id,
                by,
            },
        );
    }
}

/// Add `amount` to the investigator's `damage` and emit
/// [`Event::DamageTaken`]. Returns `true` iff the new total reaches
/// `max_health` (i.e. the investigator now qualifies for defeat under
/// [`DefeatCause::Damage`]).
///
/// Does NOT flip [`Status`] or emit [`Event::InvestigatorDefeated`] —
/// the caller composes the defeat step via [`apply_investigator_defeat`]
/// when the return is `true`. This split exists so [`enemy_attack`]
/// can place damage AND horror on the investigator before either
/// triggers defeat detection, matching the Rules Reference page 7
/// "Apply Damage/Horror" clause: *"Any assigned damage/horror that
/// has not been prevented is now placed on each card to which it has
/// been assigned, simultaneously."*
///
/// No-ops when `amount == 0` or the investigator is already defeated
/// (status `!= Active`): defeated investigators are out of play and
/// don't accumulate more damage.
///
/// [`Status`]: crate::state::Status
fn apply_damage_numeric(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) -> bool {
    if amount == 0 {
        return false;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_damage_numeric: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return false;
    }
    inv.damage = inv.damage.saturating_add(amount);
    let lethal = inv.damage >= inv.max_health;
    events.push(Event::DamageTaken {
        investigator,
        amount,
    });
    lethal
}

/// Symmetric to [`apply_damage_numeric`] but against `horror` /
/// `max_sanity`. Returns `true` iff the new total reaches the
/// max-sanity threshold; defeat application is the caller's
/// responsibility (see [`apply_investigator_defeat`]).
fn apply_horror_numeric(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) -> bool {
    if amount == 0 {
        return false;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_horror_numeric: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return false;
    }
    inv.horror = inv.horror.saturating_add(amount);
    let lethal = inv.horror >= inv.max_sanity;
    events.push(Event::HorrorTaken {
        investigator,
        amount,
    });
    lethal
}

/// Flip an Active investigator's status to the appropriate defeated
/// variant for `cause`, emit [`Event::InvestigatorDefeated`], and run
/// [`check_all_defeated`]. No-op if the investigator is already
/// non-Active (an investigator can only be defeated once per attack).
///
/// [`Status::Killed`]: crate::state::Status::Killed
/// [`Status::Insane`]: crate::state::Status::Insane
fn apply_investigator_defeat(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    cause: DefeatCause,
) {
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_investigator_defeat: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return;
    }
    inv.status = match cause {
        DefeatCause::Damage => Status::Killed,
        DefeatCause::Horror => Status::Insane,
        DefeatCause::Resigned => Status::Resigned,
    };
    events.push(Event::InvestigatorDefeated {
        investigator,
        cause,
    });
    check_all_defeated(state, events);
}

/// Apply `amount` horror to an investigator. If their accumulated
/// horror reaches `max_sanity`, flip status to [`Status::Insane`],
/// emit [`Event::InvestigatorDefeated`], and (if no `Active`
/// investigators remain) emit [`Event::AllInvestigatorsDefeated`].
///
/// No-ops when `amount == 0` or the investigator is already defeated.
///
/// Single-source horror application (currently the Draw-from-empty-
/// deck penalty) funnels through this convenience wrapper. Callers
/// that need to apply both damage AND horror from the SAME source
/// with simultaneous-placement semantics (i.e. [`enemy_attack`] and
/// any future card effect that deals both) compose the lower-level
/// [`apply_damage_numeric`] + [`apply_horror_numeric`] +
/// [`apply_investigator_defeat`] triple instead. A `take_damage`
/// twin is not provided because no single-source-damage caller exists
/// yet; the recipe (numeric helper + defeat application on `true`
/// return) is one line per call site.
///
/// [`Status::Insane`]: crate::state::Status::Insane
fn take_horror(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if apply_horror_numeric(state, events, investigator, amount) {
        apply_investigator_defeat(state, events, investigator, DefeatCause::Horror);
    }
}

/// Emit [`Event::AllInvestigatorsDefeated`] when no `Active`
/// investigator remains.
///
/// **Contract for callers:** *any* code path that flips a
/// `Status::Active` investigator to a non-`Active` status (Killed,
/// Insane, Resigned) must call this helper afterwards. Currently the
/// only status-flipping path is [`apply_investigator_defeat`], so
/// that one helper is the only caller; future paths that flip status
/// outside this helper (a scenario effect that bypasses the standard
/// defeat-cause routing) need to add a call too — otherwise the event
/// silently fails to fire when those paths cause the last `Active`
/// to fall.
///
/// Idempotent on subsequent defeats: the predicate becomes true once
/// and stays true. Callers only invoke it after a status flip, so it
/// fires exactly once per scenario in practice.
fn check_all_defeated(state: &GameState, events: &mut Vec<Event>) {
    let any_active = state
        .investigators
        .values()
        .any(|inv| inv.status == Status::Active);
    // Empty-investigators is nonsense scenario state; suppress the
    // event so we don't emit a meaningless "all defeated" when there
    // was nobody to defeat in the first place.
    if !any_active && !state.investigators.is_empty() {
        events.push(Event::AllInvestigatorsDefeated);
    }
}

/// Apply an enemy's attack pattern (damage + horror) to an
/// investigator. Used by attacks of opportunity today; will be reused
/// by the enemy-phase handler (#71) when that lands.
///
/// Per the Rules Reference, an enemy making an attack of opportunity
/// does NOT exhaust. Enemy-phase attacks DO exhaust the attacker.
/// This helper therefore does NOT touch the attacker's `exhausted`
/// flag — callers that need exhaustion (i.e. the enemy phase) apply
/// it separately.
///
/// Damage and horror are placed on the investigator **simultaneously**
/// per Rules Reference page 7 ("Apply Damage/Horror"): *"Any assigned
/// damage/horror that has not been prevented is now placed on each
/// card to which it has been assigned, simultaneously. … After
/// applying damage/horror, if an investigator has damage equal to or
/// higher than his or her health or horror equal to or higher than
/// his or her sanity, he or she is defeated."* So `inv.damage` and
/// `inv.horror` BOTH update before any defeat check, even when one
/// alone would be lethal — campaign-log accounting needs both numeric
/// values to land. Only one [`Event::InvestigatorDefeated`] fires per
/// attack regardless of how many stats crossed.
///
/// Tie-break when both stats cross simultaneously: [`DefeatCause::Damage`].
/// Per Rules Reference page 6, an investigator simultaneously defeated
/// by damage and horror *"chooses which type of trauma to suffer"* —
/// physical vs. mental in the campaign log, and the corresponding
/// in-scenario status flip follows. The engine doesn't model campaign
/// trauma yet and has no [`AwaitingInput`] prompt for "pick trauma
/// type," so `DefeatCause::Damage` is a deterministic placeholder for
/// the status flip. Route the choice through `AwaitingInput` (and pick
/// the corresponding [`Status`] variant) when trauma lands; out of
/// scope for `#83`.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
fn enemy_attack(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
    investigator: InvestigatorId,
) {
    let enemy = state.enemies.get(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "enemy_attack: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    });
    let damage = enemy.attack_damage;
    let horror = enemy.attack_horror;

    let damage_lethal = apply_damage_numeric(state, events, investigator, damage);
    let horror_lethal = apply_horror_numeric(state, events, investigator, horror);
    if damage_lethal || horror_lethal {
        let cause = if damage_lethal {
            DefeatCause::Damage
        } else {
            DefeatCause::Horror
        };
        apply_investigator_defeat(state, events, investigator, cause);
    }
}

/// Fire attacks of opportunity from every ready enemy engaged with
/// `investigator`. Each attacker resolves via [`enemy_attack`]; order
/// is deterministic by `EnemyId` (`BTreeMap` iteration).
///
/// Per the Rules Reference, the active player chooses the order of
/// `AoOs` from multiple engaged ready enemies; v1 uses deterministic
/// `EnemyId` order. We'll revisit when an actual ordering choice
/// matters (e.g. a card reacts to "the second attack of opportunity
/// this turn").
fn fire_attacks_of_opportunity(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let attackers: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator) && !e.exhausted)
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in attackers {
        enemy_attack(state, events, enemy_id, investigator);
    }
}

/// Reshuffle the discard pile back into the deck for the named
/// investigator. Used by [`draw`] when the deck runs empty. Drains
/// `discard` into `deck`, then calls [`shuffle_player_deck`] (which
/// emits [`Event::DeckShuffled`] when ≥ 2 cards land in the deck).
fn reshuffle_discard_into_deck(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "reshuffle_discard_into_deck: investigator {investigator:?} is not in the \
             investigators map; this is a state-corruption invariant violation"
            )
        });
    let cards: Vec<_> = inv.discard.drain(..).collect();
    inv.deck.extend(cards);
    shuffle_player_deck(state, events, investigator);
}

/// Handler for [`PlayerAction::Draw`].
///
/// Validate-first: Investigation phase, investigator is active and
/// `Status::Active`, has at least 1 action remaining. Then spend the
/// action and resolve the draw per the Rules Reference:
///
/// - **Non-empty deck**: draw 1 to hand.
/// - **Empty deck, non-empty discard**: shuffle discard into deck,
///   draw 1, then take 1 horror — the horror penalty fires when an
///   investigator with an empty deck needs to draw.
/// - **Both empty**: no shuffle (per the Rules Reference's "any
///   ability that would shuffle a discard pile of zero cards back
///   into a deck does not shuffle the deck"), no card drawn — but
///   the 1 horror still applies. The rules don't explicitly address
///   this corner case; we apply the horror as the safer reading
///   ("would-draw-from-empty triggers the penalty"), and the case
///   is rare enough in practice (only high-cycle decks burn through
///   both zones) that the difference is mostly theoretical.
fn draw(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "Draw: active_investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Draw: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Draw requires at least 1 action point".into(),
        };
    }

    // Mutate.
    spend_one_action(state, events, investigator);

    let inv = state.investigators.get(&investigator).expect("checked");
    let deck_empty = inv.deck.is_empty();
    let discard_empty = inv.discard.is_empty();
    if deck_empty {
        if !discard_empty {
            reshuffle_discard_into_deck(state, events, investigator);
        }
        // After the (possibly no-op) reshuffle, attempt the draw.
        // draw_cards handles a still-empty deck by emitting
        // CardsDrawn { count: 0 } without moving cards.
        draw_cards(state, events, investigator, 1);
        // Horror penalty fires on any "would-draw-from-empty-deck"
        // (the reshuffle did happen if discard was non-empty; if it
        // was also empty, the rules don't strictly require horror
        // but we apply it as the safer reading).
        take_horror(state, events, investigator, 1);
    } else {
        draw_cards(state, events, investigator, 1);
    }
    EngineOutcome::Done
}

/// Handler for [`PlayerAction::Mulligan`].
///
/// Per the Rules Reference, the redrawn cards shuffle directly back
/// into the deck (not via the discard pile). Validates the mulligan
/// window is open, the investigator is Active and hasn't already
/// mulliganed, and the redraw indices are in bounds and unique.
///
/// On success: move named hand cards to the deck, shuffle, draw the
/// same count back, set `mulligan_used = true`, emit
/// `MulliganPerformed`. An empty `indices_to_redraw` is a legal
/// "keep my hand" mulligan that consumes the one-shot without
/// touching the deck.
fn mulligan(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    indices_to_redraw: &[u8],
) -> EngineOutcome {
    if !state.mulligan_window {
        return EngineOutcome::Rejected {
            reason: "Mulligan: setup window has closed (every investigator has already \
                     mulliganed and normal play has begun)"
                .into(),
        };
    }
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: investigator {investigator:?} is not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Mulligan: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.mulligan_used {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: {investigator:?} has already used their mulligan").into(),
        };
    }
    // Validate indices: each must be in bounds and unique.
    let hand_len = inv.hand.len();
    for &idx in indices_to_redraw {
        if usize::from(idx) >= hand_len {
            return EngineOutcome::Rejected {
                reason: format!("Mulligan: hand_index {idx} out of bounds (hand size {hand_len})")
                    .into(),
            };
        }
    }
    let mut sorted: Vec<usize> = indices_to_redraw.iter().map(|&i| usize::from(i)).collect();
    sorted.sort_unstable();
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return EngineOutcome::Rejected {
            reason: format!("Mulligan: duplicate index in {indices_to_redraw:?}").into(),
        };
    }

    // Mutate.
    let redrawn_count = u8::try_from(indices_to_redraw.len())
        .expect("indices_to_redraw.len() <= hand.len() <= u8::MAX in practice");
    let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
    // Walk indices high-to-low so smaller positions remain valid as
    // we remove. Move named cards directly into the deck — they
    // shuffle back in per the rules, not through the discard pile.
    for &i in sorted.iter().rev() {
        let card = inv_mut.hand.remove(i);
        inv_mut.deck.push(card);
    }
    inv_mut.mulligan_used = true;
    // If anything actually moved, shuffle the deck (which now contains
    // the redrawn cards mixed with the rest) and draw replacements.
    // For an empty "keep my hand" mulligan, skip both — there's
    // nothing to put back, so no shuffle and no draw.
    if redrawn_count > 0 {
        shuffle_player_deck(state, events, investigator);
        draw_cards(state, events, investigator, redrawn_count);
    }
    events.push(Event::MulliganPerformed {
        investigator,
        redrawn_count,
    });
    EngineOutcome::Done
}

/// Internal helper: where a played card lands after on-play effects
/// resolve. Mirrors the Arkham rule that assets stay in play while
/// events resolve and go to the discard.
enum PlayDestination {
    /// Card stays in play (asset).
    InPlay,
    /// Card moves to the discard after on-play effects resolve (event).
    Discard,
}

/// Resolve the card's destination + abilities via the registry, or
/// produce the appropriate rejection.
///
/// Split out so [`play_card`] stays under the function-size lint —
/// and because the registry-side validations are conceptually
/// separate from the state-side prefix.
fn resolve_play_target(
    code: &CardCode,
) -> Result<(PlayDestination, Vec<crate::dsl::Ability>, bool, CardType), EngineOutcome> {
    let Some(registry) = card_registry::current() else {
        return Err(EngineOutcome::Rejected {
            reason: "PlayCard: no card registry installed; engine cannot resolve card \
                     metadata or abilities. Install game_core::card_registry before \
                     dispatching PlayCard."
                .into(),
        });
    };
    let Some(metadata) = (registry.metadata_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("PlayCard: unknown card code {code}").into(),
        });
    };
    let is_fast = metadata.is_fast;
    let card_type = metadata.card_type;
    let destination = match card_type {
        CardType::Asset => PlayDestination::InPlay,
        CardType::Event => PlayDestination::Discard,
        other => {
            return Err(EngineOutcome::Rejected {
                reason: format!(
                    "PlayCard: card_type {other:?} is not playable from hand (card {code})",
                )
                .into(),
            });
        }
    };
    let Some(abilities) = (registry.abilities_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: card {code} has no effect implementation; the deck-import \
                 gate (#73-era) should refuse decks containing unimplemented cards.",
            )
            .into(),
        });
    };
    Ok((destination, abilities, is_fast, card_type))
}

/// Handler for [`PlayerAction::PlayCard`].
///
/// Validates the standard player-action prefix, looks up the card's
/// metadata and abilities via the installed [`card_registry`], routes
/// the card to its destination zone based on its
/// [`CardType`](crate::card_data::CardType), and runs every
/// [`Trigger::OnPlay`] ability through the DSL evaluator.
///
/// # Timing gate
///
/// The gate branches on `is_fast` (from [`CardMetadata`](crate::card_data::CardMetadata))
/// and [`CardType`](crate::card_data::CardType), per Rules Reference p. 11:
///
/// - **Non-Fast cards** (asset or event without the ⚡ icon): require
///   Investigation phase + the active investigator. The standard
///   "your turn, your action" constraint.
///
/// - **Fast events** (Rules Reference p. 11: *"A fast event card may be
///   played from a player's hand any time its play instructions
///   specify"*): permitted when `active_during_investigation` OR when
///   the top open window's `fast_actors` scope permits the acting
///   investigator. Any eligible investigator in a permissive window
///   qualifies — card-level "Play only during your turn" constraints
///   (e.g. Working a Hunch 01037) are a separate per-card concern
///   **not** enforced here.
///
/// - **Fast assets** (Rules Reference p. 11: *"A fast asset may be
///   played by an investigator during any player window on his or her
///   turn"*): the "his or her turn" clause restricts to the **owner**,
///   modeled as the active investigator. Permitted when
///   `active_during_investigation` OR when the owner is the active
///   investigator AND the top open window permits them. Non-owner plays
///   remain illegal even in a permissive window.
///
/// Card-level play constraints (e.g. "Play only during your turn",
/// "Play only if …") are **not** enforced by this gate; they are a
/// future per-card concern.
///
/// # Ordering
///
/// [`Event::CardPlayed`] fires first (the play *causes* any on-play
/// effects, so it's correct for the play event to precede the
/// effects' own events in the stream). Then each [`Trigger::OnPlay`]
/// ability runs through [`apply_effect`]; if any returns non-`Done`,
/// the handler propagates that outcome. Finally the card moves out
/// of `hand` — into `cards_in_play` for assets / investigators, or
/// into `discard` (with an emitted [`Event::CardDiscarded`]) for
/// events.
///
/// # State-mutation contract caveat
///
/// For the Phase-3-scoped Core cards the on-play effects in scope
/// (`DiscoverClue`, `GainResources`) can't reject after the standard
/// validation prefix passes. If a future on-play effect can reject
/// mid-resolution, the partial mutation between [`Event::CardPlayed`]
/// and the destination move violates the engine's "no state change on
/// rejection" contract. The apply loop's belt-and-suspenders
/// `events.clear()` still clears the event stream on a rejected
/// outcome; the state-rollback hardening is out of scope here.
fn play_card(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    hand_index: u8,
) -> EngineOutcome {
    // Validate the investigator's presence and status before any card lookup.
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("PlayCard: investigator {investigator:?} is not in state").into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    let idx = usize::from(hand_index);
    if idx >= inv.hand.len() {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: hand_index {hand_index} out of bounds (hand size {})",
                inv.hand.len(),
            )
            .into(),
        };
    }
    let code: CardCode = inv.hand[idx].clone();
    // Resolve card type and abilities (also yields is_fast + card_type) before
    // applying the phase/active-investigator gate so the gate can branch on
    // is_fast AND card_type per the Rules Reference (p. 11).
    let (destination, abilities, is_fast, card_type) = match resolve_play_target(&code) {
        Ok(v) => v,
        Err(reject) => return reject,
    };
    // Timing gate — see doc-comment "# Timing gate" section above.
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
        return EngineOutcome::Rejected {
            reason: format!(
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
            .into(),
        };
    }

    // Mutate.
    events.push(Event::CardPlayed {
        investigator,
        code: code.clone(),
    });
    let ctx = EvalContext::for_controller(investigator);
    for ability in abilities.iter().filter(|a| a.trigger == Trigger::OnPlay) {
        let outcome = apply_effect(state, events, &ability.effect, ctx);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }
    }
    match destination {
        PlayDestination::InPlay => {
            let instance_id = CardInstanceId(state.next_card_instance_id);
            state.next_card_instance_id = state.next_card_instance_id.saturating_add(1);
            let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
            let card = inv_mut.hand.remove(idx);
            inv_mut
                .cards_in_play
                .push(CardInPlay::enter_play(card, instance_id));
        }
        PlayDestination::Discard => {
            let inv_mut = state.investigators.get_mut(&investigator).expect("checked");
            let card = inv_mut.hand.remove(idx);
            inv_mut.discard.push(card.clone());
            events.push(Event::CardDiscarded {
                investigator,
                code: card,
                from: Zone::Hand,
            });
        }
    }
    EngineOutcome::Done
}

/// Handler for [`PlayerAction::ActivateAbility`].
///
/// Validates the named card instance, the indexed ability's trigger,
/// and every cost-payability precondition. On success, pays every cost
/// (emitting cost events per primitive), emits [`Event::AbilityActivated`],
/// and dispatches the ability's effect through the DSL evaluator.
///
/// # Timing gate
///
/// The gate branches on `action_cost` from `Trigger::Activated`:
///
/// - **Action-cost abilities** (`action_cost > 0`): require Investigation
///   phase + active investigator + sufficient actions remaining. These consume
///   one of the investigator's limited per-turn actions.
/// - **Fast abilities** (`action_cost == 0`): per the Rules Reference, "Fast
///   abilities may be used at any player window." This handler permits them
///   when either (a) the acting investigator is the active investigator during
///   the Investigation phase, or (b) an open window's `fast_actors` scope
///   permits the acting investigator. The `open_windows` stack is pushed by
///   callers (scenario/server) when a player window opens.
///
/// # Cost coverage
///
/// - [`Cost::Resources`](crate::dsl::Cost::Resources): validates
///   wallet, deducts on payment, emits [`Event::ResourcesPaid`].
/// - [`Cost::Exhaust`](crate::dsl::Cost::Exhaust): validates source
///   not already exhausted, flips `cards_in_play[i].exhausted`,
///   emits [`Event::CardExhausted`].
/// - [`Cost::DiscardCardFromHand`](crate::dsl::Cost::DiscardCardFromHand):
///   rejects with a TODO — target-card selection needs an engine
///   `AwaitingInput` producer + `ResolveInput` dispatch. No card on
///   the roadmap uses this cost yet, so the consumer hasn't landed.
///   Test-side seam is [`ChoiceResolver`](crate::test_support::ChoiceResolver).
///
/// # State-mutation contract
///
/// Same caveat as `play_card`: costs are paid and `AbilityActivated`
/// is emitted before `apply_effect` runs, so a mid-resolution
/// rejection inside the effect leaves the costs paid. The apply
/// loop's belt-and-suspenders `events.clear()` still wipes the event
/// stream on rejection. Phase-3 in-scope effects (`GainResources`,
/// `DiscoverClue`, `Seq` of those, future `Modify`/`ThisSkillTest`
/// push) can't reject mid-flight once the standard prefix passes.
fn activate_ability(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    let Some(inv) = state.investigators.get(&investigator) else {
        return EngineOutcome::Rejected {
            reason: format!("ActivateAbility: investigator {investigator:?} is not in state")
                .into(),
        };
    };
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    let Some(in_play_pos) = inv
        .cards_in_play
        .iter()
        .position(|c| c.instance_id == instance_id)
    else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {investigator:?} has no in-play instance {instance_id:?}",
            )
            .into(),
        };
    };
    let source_code = inv.cards_in_play[in_play_pos].code.clone();
    let source_exhausted = inv.cards_in_play[in_play_pos].exhausted;

    let (action_cost, costs, effect) = match resolve_activated_ability(&source_code, ability_index)
    {
        Ok(v) => v,
        Err(reject) => return reject,
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
            return EngineOutcome::Rejected {
                reason: format!(
                    "ActivateAbility: action-cost ability requires Investigation phase + \
                     active investigator (phase was {:?}, active {:?})",
                    state.phase, state.active_investigator,
                )
                .into(),
            };
        }
    } else {
        // Fast ability: active during Investigation OR permissive window.
        if !active_during_investigation && !in_permissive_window {
            return EngineOutcome::Rejected {
                reason: "ActivateAbility: Fast ability requires either active investigator \
                         during Investigation, or an open window whose fast_actors permits \
                         this investigator"
                    .into(),
            };
        }
    }

    // Re-borrow inv after state borrows above.
    let inv = state.investigators.get(&investigator).expect("checked");

    // Action-economy check.
    if inv.actions_remaining < action_cost {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: needs {action_cost} action(s); investigator has {}",
                inv.actions_remaining,
            )
            .into(),
        };
    }

    // Validate every payment cost is payable. Done as a pure read
    // before any mutation so an all-or-nothing reject leaves state
    // untouched.
    for cost in &costs {
        if let Err(reason) = check_cost_payable(cost, inv, source_exhausted) {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            };
        }
    }

    // Mutate.
    pay_activation_costs(
        state,
        events,
        investigator,
        instance_id,
        in_play_pos,
        &source_code,
        action_cost,
        &costs,
    );
    events.push(Event::AbilityActivated {
        investigator,
        instance_id,
        code: source_code,
        ability_index,
    });

    let ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    apply_effect(state, events, &effect, ctx)
}

/// Pay the action cost and every payment cost of an activated
/// ability. Mutates state in place and pushes the matching events.
/// Caller has already validated that every cost is payable.
#[allow(clippy::too_many_arguments)]
fn pay_activation_costs(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    in_play_pos: usize,
    source_code: &CardCode,
    action_cost: u8,
    costs: &[Cost],
) {
    let inv_mut = state
        .investigators
        .get_mut(&investigator)
        .expect("validated above");
    if action_cost > 0 {
        inv_mut.actions_remaining = inv_mut.actions_remaining.saturating_sub(action_cost);
        events.push(Event::ActionsRemainingChanged {
            investigator,
            new_count: inv_mut.actions_remaining,
        });
    }
    for cost in costs {
        match cost {
            Cost::Resources(n) => {
                inv_mut.resources = inv_mut.resources.saturating_sub(*n);
                events.push(Event::ResourcesPaid {
                    investigator,
                    amount: *n,
                });
            }
            Cost::Exhaust => {
                inv_mut.cards_in_play[in_play_pos].exhausted = true;
                events.push(Event::CardExhausted {
                    investigator,
                    instance_id,
                    code: source_code.clone(),
                });
            }
            Cost::DiscardCardFromHand => {
                unreachable!("DiscardCardFromHand rejected earlier in check_cost_payable")
            }
        }
    }
}

/// Resolve the activated ability at `(code, ability_index)` from the
/// installed [`card_registry`], returning its `(action_cost, costs,
/// effect)` triple or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
fn resolve_activated_ability(
    code: &CardCode,
    ability_index: u8,
) -> Result<(u8, Vec<Cost>, crate::dsl::Effect), EngineOutcome> {
    let Some(registry) = card_registry::current() else {
        return Err(EngineOutcome::Rejected {
            reason: "ActivateAbility: no card registry installed; engine cannot resolve abilities."
                .into(),
        });
    };
    let Some(abilities) = (registry.abilities_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("ActivateAbility: card {code} has no effect implementation").into(),
        });
    };
    let idx = usize::from(ability_index);
    let Some(ability) = abilities.get(idx) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability_index {ability_index} out of bounds for {code} \
                 (has {} abilities)",
                abilities.len(),
            )
            .into(),
        });
    };
    let Trigger::Activated { action_cost } = ability.trigger else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability {ability_index} on {code} is not an Activated \
                 trigger (got {:?})",
                ability.trigger,
            )
            .into(),
        });
    };
    Ok((action_cost, ability.costs.clone(), ability.effect.clone()))
}

/// Validate a single [`Cost`] is currently payable against `inv` /
/// `source_exhausted`. Returns the reject reason on failure. Does
/// NOT mutate; the caller does the actual deduction after all costs
/// are checked.
fn check_cost_payable(
    cost: &Cost,
    inv: &Investigator,
    source_exhausted: bool,
) -> Result<(), String> {
    match cost {
        Cost::Resources(n) => {
            if inv.resources < *n {
                return Err(format!(
                    "ActivateAbility: needs {n} resources; investigator has {}",
                    inv.resources,
                ));
            }
            Ok(())
        }
        Cost::Exhaust => {
            if source_exhausted {
                return Err(
                    "ActivateAbility: source card is already exhausted; Exhaust cost \
                     cannot be paid"
                        .to_string(),
                );
            }
            Ok(())
        }
        Cost::DiscardCardFromHand => Err(
            "TODO: Cost::DiscardCardFromHand requires AwaitingInput + ResolveInput \
             dispatch; no card uses this cost yet so the engine consumer hasn't landed."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod encounter_deck_helper_tests {
    use super::*;
    use crate::event::Event;
    use crate::rng::RngState;
    use crate::state::CardCode;
    use crate::test_support::TestGame;

    #[test]
    fn shuffle_encounter_deck_emits_event_when_two_or_more_cards() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(42);
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();
        shuffle_encounter_deck(&mut state, &mut events);

        assert!(matches!(events.as_slice(), [Event::EncounterDeckShuffled]));
        assert_eq!(state.encounter_deck.len(), 3);
        let mut codes: Vec<_> = state.encounter_deck.iter().cloned().collect();
        codes.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(codes, vec![CardCode("a".into()), CardCode("b".into()), CardCode("c".into())]);
    }

    #[test]
    fn shuffle_encounter_deck_is_silent_on_zero_or_one_card() {
        for n in 0..=1 {
            let mut state = TestGame::new().build();
            for i in 0..n {
                state.encounter_deck.push_back(CardCode(format!("c{i}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
            assert!(events.is_empty(), "expected no event for n={n} deck");
        }
    }

    #[test]
    fn reshuffle_encounter_discard_moves_discard_into_deck_and_shuffles() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(7);
        for i in 0..5 {
            state.encounter_discard.push(CardCode(format!("d{i}")));
        }

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

        assert!(state.encounter_discard.is_empty(), "discard should be drained");
        assert_eq!(state.encounter_deck.len(), 5, "all 5 cards moved into deck");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "expected EncounterDeckShuffled (≥ 2 cards moved)"
        );
    }

    #[test]
    fn reshuffle_encounter_discard_is_silent_when_discard_has_one_card() {
        let mut state = TestGame::new().build();
        state.encounter_discard.push(CardCode("solo".into()));

        let mut events = Vec::new();
        reshuffle_encounter_discard(&mut state, &mut events);

        assert!(state.encounter_discard.is_empty());
        assert_eq!(state.encounter_deck.len(), 1);
        assert!(events.is_empty(), "1-card shuffle emits no event");
    }

    #[test]
    fn draw_encounter_top_drains_deck_then_returns_none() {
        let mut state = TestGame::new().build();
        state.encounter_deck.push_back(CardCode("a".into()));
        state.encounter_deck.push_back(CardCode("b".into()));
        state.encounter_deck.push_back(CardCode("c".into()));

        let mut events = Vec::new();

        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("a".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("b".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), Some(CardCode("c".into())));
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        assert!(events.is_empty(), "no events when draining a non-empty deck");
    }

    #[test]
    fn draw_encounter_top_reshuffles_discard_on_empty_deck() {
        let mut state = TestGame::new().build();
        state.rng = RngState::new(13);
        state.encounter_discard.push(CardCode("x".into()));
        state.encounter_discard.push(CardCode("y".into()));
        state.encounter_discard.push(CardCode("z".into()));

        let mut events = Vec::new();
        let drawn = draw_encounter_top(&mut state, &mut events);

        assert!(drawn.is_some(), "should reshuffle and draw");
        assert_eq!(state.encounter_deck.len(), 2, "2 cards remain in deck post-draw");
        assert!(state.encounter_discard.is_empty(), "discard drained");
        assert!(
            matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
            "reshuffle emits one event"
        );
    }

    #[test]
    fn draw_encounter_top_returns_none_when_deck_and_discard_both_empty() {
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        assert_eq!(draw_encounter_top(&mut state, &mut events), None);
        assert!(events.is_empty(), "no events on empty-on-both");
    }

    #[test]
    fn engine_record_encounter_deck_shuffled_drives_shuffle() {
        use crate::action::{Action, EngineRecord};
        use crate::engine::apply;

        let mut state = TestGame::new().build();
        state.rng = RngState::new(99);
        for i in 0..4 {
            state.encounter_deck.push_back(CardCode(format!("c{i}")));
        }
        let original: Vec<_> = state.encounter_deck.iter().cloned().collect();

        let result = apply(state, Action::Engine(EngineRecord::EncounterDeckShuffled));

        assert!(
            matches!(result.outcome, crate::EngineOutcome::Done),
            "expected Done, got {:?}",
            result.outcome
        );
        let mut shuffled: Vec<_> = result.state.encounter_deck.iter().cloned().collect();
        let mut orig_sorted = original.clone();
        shuffled.sort_by(|a, b| a.0.cmp(&b.0));
        orig_sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(shuffled, orig_sorted);
        assert!(result.events.iter().any(|e| matches!(e, Event::EncounterDeckShuffled)));
    }

    #[test]
    fn encounter_deck_shuffle_is_deterministic_from_seed() {
        fn shuffle_with_seed(seed: u64) -> Vec<CardCode> {
            let mut state = TestGame::new().build();
            state.rng = RngState::new(seed);
            for i in 0..10 {
                state.encounter_deck.push_back(CardCode(format!("c{i:02}")));
            }
            let mut events = Vec::new();
            shuffle_encounter_deck(&mut state, &mut events);
            state.encounter_deck.iter().cloned().collect()
        }

        let a = shuffle_with_seed(2026);
        let b = shuffle_with_seed(2026);
        assert_eq!(a, b, "same seed must produce same shuffle order");

        let c = shuffle_with_seed(42);
        assert_ne!(a, c, "different seeds should produce different orders (smoke test)");
    }
}
