//! Per-action dispatch handlers.
//!
//! Each function applies a single action variant to the state, mutating
//! the state in place and pushing the resulting events onto the events
//! buffer. Returns the [`EngineOutcome`] for the action.
//!
//! Handlers are split by `Action` bucket: [`apply_player_action`] for
//! human-initiated actions, [`apply_engine_record`] for engine-emitted
//! ones.

use crate::action::{EngineRecord, PlayerAction};
use crate::card_data::CardType;
use crate::card_registry;
use crate::dsl::{discover_clue, Cost, LocationTarget, SkillTestKind, Trigger};
use crate::event::{Event, FailureReason};
use crate::state::{
    resolve_token, CardCode, CardInPlay, CardInstanceId, DefeatCause, Enemy, EnemyId, GameState,
    Investigator, InvestigatorId, LocationId, Phase, SkillKind, Status, TokenResolution, Zone,
};

use super::evaluator::{apply_effect, constant_skill_modifier, EvalContext};
use super::outcome::EngineOutcome;

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
        PlayerAction::ResolveInput { .. } => EngineOutcome::Rejected {
            reason: "TODO(#63): ResolveInput dispatch lands with the skill-test commit \
                     window; no AwaitingInput sites exist yet."
                .into(),
        },
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

/// The outcome of resolving a skill test, when the test runs at all.
/// Returned by [`resolve_skill_test`] so callers (the
/// `PerformSkillTest` dispatch wrapper, `Investigate`, future Fight /
/// Evade) can branch on success vs failure to apply action-specific
/// follow-on effects.
///
/// `Succeeded.margin` and `Failed.{reason, by}` carry the same numbers
/// as the corresponding events; callers that don't need them can
/// match `Ok(SkillTestResolution::Succeeded { .. })`. Fields are
/// `allow(dead_code)` until Fight/Evade (which want fail-by-X logic)
/// land.
#[allow(dead_code)]
pub(super) enum SkillTestResolution {
    /// The investigator's clamped total met or exceeded the difficulty.
    Succeeded {
        /// `total - difficulty` (always `>= 0`).
        margin: i8,
    },
    /// The test failed.
    Failed {
        /// Why it failed.
        reason: FailureReason,
        /// Margin of failure (always `>= 0`).
        by: i8,
    },
}

/// Run the skill-test resolution sequence and return the outcome to
/// the caller. Pushes the bracketing `SkillTestStarted` / `…Ended`
/// events plus the per-step events (`ChaosTokenRevealed`,
/// `SkillTestSucceeded` or `SkillTestFailed`).
///
/// Returns `Err(EngineOutcome::Rejected { .. })` on validation failure
/// without pushing any events.
///
/// **`AutoFail`** forces the investigator's total to 0 per the Rules
/// Reference; **`ElderSign`** is treated as `Modifier(0)` until per-
/// investigator ability dispatch lands; **negative** `skill + modifier`
/// clamps to 0.
pub(super) fn resolve_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
    difficulty: i8,
) -> Result<SkillTestResolution, EngineOutcome> {
    // Validate-first: investigator must exist and be Active; chaos
    // bag must be non-empty so we can draw; difficulty must be non-
    // negative (FFG difficulties are always ≥ 0). Defeated
    // investigators can't take skill tests — they're out of play.
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("skill test: investigator {investigator:?} not in state").into(),
        });
    };
    if inv.status != Status::Active {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "skill test: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        });
    }
    if state.chaos_bag.tokens.is_empty() {
        return Err(EngineOutcome::Rejected {
            reason: "skill test requires a non-empty chaos bag".into(),
        });
    }
    if difficulty < 0 {
        return Err(EngineOutcome::Rejected {
            reason: format!("skill test: difficulty {difficulty} must be >= 0").into(),
        });
    }
    let base_skill = inv.skills.value(skill);
    // Drop the `inv` borrow before re-borrowing state for the
    // constant-modifier query (it walks state.investigators[*].cards_in_play).
    // The modifier contribution is 0 when no card registry is installed —
    // most engine unit tests don't install one, and a skill test with no
    // card effects in play is the correct fallback.
    let modifier = card_registry::current().map_or(0, |reg| {
        constant_skill_modifier(state, reg, investigator, skill, kind)
    });
    let skill_value = base_skill.saturating_add(modifier);

    // Mutate-second: advance RNG, derive token, emit events.
    events.push(Event::SkillTestStarted {
        investigator,
        skill,
        difficulty,
    });

    let idx = state.rng.next_index(state.chaos_bag.tokens.len());
    let token = state.chaos_bag.tokens[idx];
    let resolution = resolve_token(token, &state.token_modifiers);
    events.push(Event::ChaosTokenRevealed { token, resolution });

    // All arithmetic stays in i8 with saturating ops: realistic
    // gameplay values (skill 1–8, modifier ±8, difficulty ≤ ~6) fit
    // far inside i8, but saturation defends against absurd state
    // configurations without needing a wider integer type.
    let (total, fail_reason) = match resolution {
        TokenResolution::Modifier(n) => (skill_value.saturating_add(n).max(0), None),
        TokenResolution::ElderSign => (skill_value.max(0), None),
        TokenResolution::AutoFail => (0, Some(FailureReason::AutoFail)),
    };
    let margin = total.saturating_sub(difficulty);
    let outcome = if margin >= 0 && fail_reason.is_none() {
        events.push(Event::SkillTestSucceeded {
            investigator,
            skill,
            margin,
        });
        SkillTestResolution::Succeeded { margin }
    } else {
        let reason = fail_reason.unwrap_or(FailureReason::Total);
        let by = difficulty.saturating_sub(total);
        events.push(Event::SkillTestFailed {
            investigator,
            skill,
            reason,
            by,
        });
        SkillTestResolution::Failed { reason, by }
    };

    events.push(Event::SkillTestEnded { investigator });
    Ok(outcome)
}

/// Public dispatch wrapper for [`PlayerAction::PerformSkillTest`].
///
/// Card commits, the commit-window `AwaitingInput`, and the after-
/// resolution trigger window are downstream (#63 / #64). The skill-
/// test machinery itself lives in [`resolve_skill_test`], which other
/// turn-actions (Investigate, future Fight / Evade) invoke directly.
fn perform_skill_test(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) -> EngineOutcome {
    match resolve_skill_test(
        state,
        events,
        investigator,
        skill,
        SkillTestKind::Plain,
        difficulty,
    ) {
        Ok(_) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
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

    match resolve_skill_test(
        state,
        events,
        investigator,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        difficulty,
    ) {
        Ok(SkillTestResolution::Succeeded { .. }) => {
            let effect = discover_clue(LocationTarget::ControllerLocation, 1);
            let ctx = EvalContext::for_controller(investigator);
            // The remaining rejection path inside `discover_clue` is
            // "controller is between locations," which we already
            // validated above. Empty-location is a silent no-op by
            // design. So any rejection here is a state-corruption
            // invariant violation — surface it loudly, not as a
            // half-applied Rejected outcome.
            let outcome = apply_effect(state, events, &effect, ctx);
            if let EngineOutcome::Rejected { reason } = outcome {
                unreachable!(
                    "Investigate: discover_clue rejected unexpectedly after validation: {reason}"
                );
            }
            EngineOutcome::Done
        }
        Ok(SkillTestResolution::Failed { .. }) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
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
    match resolve_skill_test(
        state,
        events,
        investigator,
        SkillKind::Combat,
        SkillTestKind::Fight,
        fight_difficulty,
    ) {
        Ok(SkillTestResolution::Succeeded { .. }) => {
            damage_enemy(state, events, enemy_id, 1, Some(investigator));
            EngineOutcome::Done
        }
        Ok(SkillTestResolution::Failed { .. }) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
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
    match resolve_skill_test(
        state,
        events,
        investigator,
        SkillKind::Agility,
        SkillTestKind::Evade,
        evade_difficulty,
    ) {
        Ok(SkillTestResolution::Succeeded { .. }) => {
            let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
                unreachable!(
                    "Evade: enemy {enemy_id:?} disappeared between validation and resolution; \
                     this is a state-corruption invariant violation"
                )
            });
            enemy.engaged_with = None;
            enemy.exhausted = true;
            events.push(Event::EnemyDisengaged {
                enemy: enemy_id,
                investigator,
            });
            events.push(Event::EnemyExhausted { enemy: enemy_id });
            EngineOutcome::Done
        }
        Ok(SkillTestResolution::Failed { .. }) => EngineOutcome::Done,
        Err(rejected) => rejected,
    }
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
    }
}

/// Apply `amount` damage to an investigator. If their accumulated
/// damage reaches `max_health`, flip status to [`Status::Killed`],
/// emit [`Event::InvestigatorDefeated`], and (if no `Active`
/// investigators remain) emit [`Event::AllInvestigatorsDefeated`].
///
/// No-ops when `amount == 0` or the investigator is already defeated
/// (status `!= Active`): defeated investigators are out of play and
/// don't accumulate more damage.
///
/// All damage-application paths should funnel through this helper so
/// defeat detection stays consistent. The first caller is
/// [`enemy_attack`]; future damage-dealing card effects plug in here
/// too.
///
/// [`Status::Killed`]: crate::state::Status::Killed
fn take_damage(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if amount == 0 {
        return;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "take_damage: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return;
    }
    inv.damage = inv.damage.saturating_add(amount);
    let max_health = inv.max_health;
    let now_defeated = inv.damage >= max_health;
    events.push(Event::DamageTaken {
        investigator,
        amount,
    });
    if now_defeated {
        inv.status = Status::Killed;
        events.push(Event::InvestigatorDefeated {
            investigator,
            cause: DefeatCause::Damage,
        });
        check_all_defeated(state, events);
    }
}

/// Apply `amount` horror to an investigator. Symmetric to
/// [`take_damage`] but against `horror` / `max_sanity`, defeating
/// with [`Status::Insane`] and [`DefeatCause::Horror`].
///
/// [`Status::Insane`]: crate::state::Status::Insane
fn take_horror(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if amount == 0 {
        return;
    }
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "take_horror: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return;
    }
    inv.horror = inv.horror.saturating_add(amount);
    let max_sanity = inv.max_sanity;
    let now_defeated = inv.horror >= max_sanity;
    events.push(Event::HorrorTaken {
        investigator,
        amount,
    });
    if now_defeated {
        inv.status = Status::Insane;
        events.push(Event::InvestigatorDefeated {
            investigator,
            cause: DefeatCause::Horror,
        });
        check_all_defeated(state, events);
    }
}

/// Emit [`Event::AllInvestigatorsDefeated`] when no `Active`
/// investigator remains.
///
/// **Contract for callers:** *any* code path that flips a
/// `Status::Active` investigator to a non-`Active` status (Killed,
/// Insane, Resigned) must call this helper afterwards. Currently
/// only [`take_damage`] / [`take_horror`] flip status, so they're
/// the only callers; future paths (the Resign action, scenario
/// effects that defeat directly) need to add a call too — otherwise
/// the event silently fails to fire when those paths cause the last
/// `Active` to fall.
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
/// Defeat handling flows through [`take_damage`] / [`take_horror`].
/// If damage defeats first, horror is a no-op (already defeated);
/// per rules damage and horror from an attack are simultaneous, so
/// the practical loss of information is minor (the cause field on
/// [`Event::InvestigatorDefeated`] still identifies the lethal
/// stat).
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

    take_damage(state, events, investigator, damage);
    take_horror(state, events, investigator, horror);
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
) -> Result<(PlayDestination, Vec<crate::dsl::Ability>), EngineOutcome> {
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
    let destination = match metadata.card_type {
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
    Ok((destination, abilities))
}

/// Handler for [`PlayerAction::PlayCard`].
///
/// Validates the standard player-action prefix, looks up the card's
/// metadata and abilities via the installed [`card_registry`], routes
/// the card to its destination zone based on its
/// [`CardType`](crate::card_data::CardType), and runs every
/// [`Trigger::OnPlay`] ability through the DSL evaluator.
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
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard is only valid during the Investigation phase (was {:?})",
                state.phase,
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "PlayCard: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
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
    let (destination, abilities) = match resolve_play_target(&code) {
        Ok(v) => v,
        Err(reject) => return reject,
    };

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
/// Validates the standard player-action prefix, the named card
/// instance, the indexed ability's trigger, and every cost-payability
/// precondition. On success, pays every cost (emitting cost events
/// per primitive), emits [`Event::AbilityActivated`], and dispatches
/// the ability's effect through the DSL evaluator.
///
/// # Cost coverage
///
/// - [`Cost::Resources`](crate::dsl::Cost::Resources): validates
///   wallet, deducts on payment, emits [`Event::ResourcesPaid`].
/// - [`Cost::Exhaust`](crate::dsl::Cost::Exhaust): validates source
///   not already exhausted, flips `cards_in_play[i].exhausted`,
///   emits [`Event::CardExhausted`].
/// - [`Cost::DiscardCardFromHand`](crate::dsl::Cost::DiscardCardFromHand):
///   rejects with a TODO — target-card selection needs the
///   `ChoiceResolver` (#19).
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
///
/// # Known simplification: Investigation-phase + active-investigator gate
///
/// This handler requires the acting investigator to be the active
/// one during the Investigation phase. That's correct for
/// `[action]`-cost abilities, but **overly strict for `[fast]` ones**
/// (`Trigger::Activated { action_cost: 0 }`): in real Arkham, Fast
/// abilities are legal in any player window — between phases,
/// during another investigator's turn, between enemy attacks. No
/// phase outside Investigation exists yet, so this simplification
/// is a no-op for Phase-3 scope; #103 lifts the gate when phase
/// content (#69 / #70 / #71) lands.
fn activate_ability(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    if state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility is only valid during the Investigation phase (was {:?})",
                state.phase,
            )
            .into(),
        };
    }
    if state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        };
    }
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

    let ctx = EvalContext::for_controller(investigator);
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
            "TODO(#19): Cost::DiscardCardFromHand requires AwaitingInput plumbing + \
             ResolveInput; lands with the ChoiceResolver."
                .to_string(),
        ),
    }
}
