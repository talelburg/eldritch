//! Hunter-movement and prey-resolution helpers (Enemy phase step 3.2).

use std::fmt::Debug;

use card_dsl::card_data::{Prey, PreyDirection, PreyMeasure};

use crate::action::InputResponse;
use crate::card_registry::{self, CardRegistry};
use crate::engine::dispatch::{choice, cursor, movement, phases};
use crate::engine::modified_value::{self, ModifiedQuantity, ModifierTarget, ReadContext};
use crate::engine::outcome::{EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken};
use crate::engine::{pathfinding, Cx};
use crate::event::Event;
use crate::state::{
    Continuation, Enemy, EnemyId, GameState, HunterChoice, Investigator, InvestigatorId,
    LocationId, SpawnEngagePending, Status,
};

/// Result of narrowing a candidate investigator set by a prey
/// instruction (Rules Reference p.12 / p.17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreyResolution {
    /// Exactly one investigator best meets the instruction.
    One(InvestigatorId),
    /// Two or more tie — the lead investigator decides (carries the
    /// tied set, in input order).
    Tie(Vec<InvestigatorId>),
    /// No candidates at all.
    None,
}

/// The value an investigator scores for a comparative prey
/// [`PreyMeasure`]. Widened to `i32` so `base_health − damage` can't
/// underflow and so skills/health share one comparable type. Higher or
/// lower is selected by the [`PreyDirection`] in [`Prey::Ranked`].
///
/// The *modified* value (Rules Reference p.18 Modifiers, p.12 remaining
/// health): the [modified value](modified_value) of the underlying
/// quantity, less any damage taken for remaining health, floored at zero
/// (RR p.15 — a stat cannot function below zero). The floor is applied
/// again after the subtraction, so an investigator whose damage exceeds
/// their modified max health ranks at 0 rather than below it.
///
/// Prey resolves outside any skill test, so the read passes
/// [`ReadContext::OutsideTest`] explicitly: only unconditional
/// (`WhileInPlay`) modifiers apply, whatever happens to be in flight
/// elsewhere. `registry` is `None` in engine-only tests with no card
/// data installed (base values only).
fn measure_value(
    state: &GameState,
    registry: Option<&CardRegistry>,
    inv: &Investigator,
    measure: PreyMeasure,
) -> i32 {
    // One match, so the quantity asked for and the damage subtracted
    // from it can't drift apart: a skill is measured whole, remaining
    // health is max health less the damage taken.
    let (quantity, damage) = match measure {
        PreyMeasure::Skill(kind) => (ModifiedQuantity::Skill(kind), 0),
        PreyMeasure::RemainingHealth => (ModifiedQuantity::MaxHealth, i32::from(inv.damage())),
    };
    let value = modified_value::modified_value(
        state,
        registry,
        ModifierTarget::Investigator(inv.id),
        quantity,
        ReadContext::OutsideTest,
    )
    .total();
    (value - damage).max(0)
}

/// Narrow `candidates` by `prey`. `Default` treats all candidates as
/// equal; `Ranked` keeps those with the most extreme measure value (max
/// for `Highest`, min for `Lowest`). Returns `One` (single best), `Tie`
/// (2+ best — lead decides), or `None` (empty candidate set). Caller
/// supplies the candidate set (equidistant-nearest investigators for
/// movement; co-located investigators for engagement).
pub(super) fn resolve_prey(
    state: &GameState,
    prey: Prey,
    candidates: &[InvestigatorId],
) -> PreyResolution {
    if candidates.is_empty() {
        return PreyResolution::None;
    }
    let registry = card_registry::current();
    let best: Vec<InvestigatorId> = match prey {
        Prey::Default => candidates.to_vec(),
        Prey::Ranked { direction, measure } => {
            let scored: Vec<(InvestigatorId, i32)> = candidates
                .iter()
                .filter_map(|id| {
                    state
                        .investigators
                        .get(id)
                        .map(|inv| (*id, measure_value(state, registry, inv, measure)))
                })
                .collect();
            let extreme = match direction {
                PreyDirection::Highest => scored.iter().map(|(_, v)| *v).max(),
                PreyDirection::Lowest => scored.iter().map(|(_, v)| *v).min(),
            };
            match extreme {
                Some(target) => scored
                    .iter()
                    .filter(|(_, v)| *v == target)
                    .map(|(id, _)| *id)
                    .collect(),
                None => Vec::new(),
            }
        }
        // `Prey` is #[non_exhaustive]; new *comparative* measures are
        // added to `PreyMeasure` (compile-forced — exhaustive), so this
        // arm only guards genuinely new non-`Ranked` shapes (e.g.
        // "Bearer only"), which must be wired here when they land.
        _ => unreachable!(
            "resolve_prey: unrecognised Prey variant {prey:?} — \
             card-impl bug or new variant needs engine wiring"
        ),
    };
    match best.as_slice() {
        [] => PreyResolution::None,
        [one] => PreyResolution::One(*one),
        _ => PreyResolution::Tie(best),
    }
}

/// Whether an enemy is an eligible hunter for step-3.2 movement:
/// ready, unengaged, has the keyword, and is on the map.
fn is_eligible_hunter(enemy: &Enemy) -> bool {
    enemy.hunter
        && !enemy.exhausted
        && enemy.engaged_with.is_none()
        && enemy.current_location.is_some()
}

/// Compute the prey-legal destination set for a hunter at `from`:
/// the union of shortest-path first-steps toward each
/// equidistant-nearest, prey-filtered investigator. Deterministic order
/// (sorted `LocationId`). Empty means the hunter does not move — either no
/// investigator is reachable, or every compelled step is blocked.
///
/// A movement block is applied to the *step*, never to the graph (#651).
/// `data/rules-reference/rules/glossary/Nearest.md`: *"Nearest refers to the
/// entity of the specified kind at a location that can be reached in the
/// fewest number of connections, even if one or more of those connections
/// are blocked by another card ability. The path to the nearest entity is
/// the 'shortest' path to that entity."* So distances and shortest paths are
/// measured on the full connection graph — a barricade never changes who
/// is nearest or which way is shortest — and only the resulting first steps
/// are filtered through [`enemy_can_enter_location`], per
/// `data/rules-reference/rules/glossary/Hunter.md`: *"If a hunter enemy would
/// be compelled to a location to which the move is blocked by a card ability,
/// the enemy does not move."*
///
/// Where several shortest first steps tie, dropping the blocked ones is the
/// reading taken here: the enemy is compelled to the *set*, and a step is only
/// "the" compelled one once it has been chosen, so an unblocked tied step
/// stays available. This is filtering, not rerouting — every surviving step is
/// still a shortest first step on the true graph, and when every tied step is
/// blocked the set empties and the enemy does not move.
///
/// The filter runs over the union across prey-tied investigators, so a blocked
/// step toward one tied investigator can still leave an unblocked step toward
/// another. Under a stricter reading the lead's choice is over *investigators*
/// first — pick the blocked one and the hunter stays put — but this engine
/// offers the lead destinations rather than investigators (#128), so that
/// reading has nowhere to live until the choice is remodelled (#795).
fn hunter_destinations(
    state: &GameState,
    from: LocationId,
    prey: Prey,
    enemy: &Enemy,
) -> Vec<LocationId> {
    let mut reachable: Vec<(InvestigatorId, u32)> = Vec::new();
    let mut min_dist: Option<u32> = None;
    for id in &state.turn_order {
        let Some(inv) = state.investigators.get(id) else {
            continue;
        };
        if inv.status != Status::Active {
            continue;
        }
        let Some(loc) = inv.current_location else {
            continue;
        };
        let Some(d) = pathfinding::bfs_distance(state, from, loc) else {
            continue;
        };
        min_dist = Some(min_dist.map_or(d, |m| m.min(d)));
        reachable.push((*id, d));
    }
    let Some(min) = min_dist else {
        return Vec::new();
    };
    let nearest_ids: Vec<InvestigatorId> = reachable
        .iter()
        .filter(|(_, d)| *d == min)
        .map(|(id, _)| *id)
        .collect();
    let chosen: Vec<InvestigatorId> = match resolve_prey(state, prey, &nearest_ids) {
        PreyResolution::One(id) => vec![id],
        PreyResolution::Tie(v) => v,
        PreyResolution::None => return Vec::new(),
    };
    let mut dests: Vec<LocationId> = Vec::new();
    for id in chosen {
        let Some(loc) = state
            .investigators
            .get(&id)
            .and_then(|i| i.current_location)
        else {
            continue;
        };
        for step in pathfinding::shortest_first_steps(state, from, loc) {
            // The block bites here and only here: a barricaded step is one the
            // enemy cannot be compelled into, so it drops out of the offered
            // set rather than out of the graph the distances were measured on.
            if movement::enemy_can_enter_location(state, enemy, step) && !dests.contains(&step) {
                dests.push(step);
            }
        }
    }
    dests.sort();
    dests
}

/// Write `enemy`'s position and emit [`Event::EnemyMoved`] — the board
/// write alone, with no engagement check. Caller has already validated
/// that `to` is a legal destination.
///
/// The only two callers are [`relocate_enemy`] (the funnel every mover
/// should use) and the Hunter path, which runs its own *interactive*
/// [`engage_on_arrival`] immediately afterwards: a prey tie there
/// suspends for the lead's choice rather than auto-picking, so it must
/// not go through `relocate_enemy`'s synchronous engagement.
fn place_enemy_at(cx: &mut Cx, enemy_id: EnemyId, to: LocationId) {
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("place_enemy_at: enemy {enemy_id:?} vanished mid-movement; state corruption")
    });
    enemy.current_location = Some(to);
    cx.events.push(Event::EnemyMoved {
        enemy: enemy_id,
        to,
    });
}

/// Move an enemy to `to` and run the engage-on-arrival check there —
/// the funnel for every card effect, scenario native, or engine path
/// that relocates an enemy (#633).
///
/// `data/rules-reference/rules/glossary/Enemy_Engagement.md`: *"Any time
/// a ready unengaged enemy is at the same location as an investigator,
/// it engages that investigator, and is placed in that investigator's
/// threat area"*, and among the listed examples: *"It moves into the
/// same location as an investigator"*. The check is
/// `reengage_at_location`, so its rules already hold here: an
/// exhausted enemy does not engage (*"An exhausted unengaged enemy does
/// not engage"* — agenda 01107 *can* move evaded enemies, and they
/// arrive unengaged), and prey resolution picks among co-located
/// investigators.
///
/// **Takes an unengaged enemy.** An enemy that is already engaged is
/// left engaged — `reengage_at_location`'s precondition is
/// `engaged_with == None`, so the check is skipped rather than
/// re-targeting it. That is right for the only engaged-enemy relocation
/// the engine has (`move_action` dragging an enemy along with the
/// investigator it is engaged with, which is why that path does not
/// call this one), but relocating an engaged enemy *away* from its
/// investigator would strand the engagement across two locations,
/// against the same glossary paragraph: *"Each enemy in an
/// investigator's threat area is considered to be at the same location
/// as that investigator"*. Disengage first (emitting
/// [`Event::EnemyDisengaged`]) if a future effect needs that.
///
/// Enemy *spawning* is not relocation and does not come through here:
/// `spawn_enemy_at` mints the enemy already at its location and runs
/// its own prey resolution, so the `EnemySpawned` event precedes the
/// `EnemyEngaged` one and a spawn-time tie can suspend interactively.
///
/// Synchronous by construction, unlike the Hunter path: a prey tie
/// among co-located investigators auto-picks the turn-order-first lead
/// rather than suspending for the lead's choice (see
/// `reengage_at_location`'s `TODO(#151)`). Solo play — the only mode
/// that ships today — never ties.
pub fn relocate_enemy(cx: &mut Cx, enemy_id: EnemyId, to: LocationId) {
    place_enemy_at(cx, enemy_id, to);
    if cx.state.enemies[&enemy_id].engaged_with.is_none() {
        reengage_at_location(cx, enemy_id);
    }
}

/// Set engagement on `enemy_id` → `target` and emit
/// [`Event::EnemyEngaged`]. Shared by movement and spawn.
pub(super) fn engage_enemy_with(cx: &mut Cx, enemy_id: EnemyId, target: InvestigatorId) {
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("engage_enemy_with: enemy {enemy_id:?} vanished; state corruption")
    });
    enemy.engaged_with = Some(target);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator: target,
    });
}

/// Engage-on-arrival for a hunter now at its (possibly unchanged)
/// location. Returns `Some(HunterChoice::Engage{..})` if the co-located
/// set ties under prey (caller suspends); otherwise engages the resolved
/// investigator (or no-one) and returns `None`.
fn engage_on_arrival(cx: &mut Cx, enemy_id: EnemyId) -> Option<HunterChoice> {
    let loc = cx.state.enemies[&enemy_id]
        .current_location
        .unwrap_or_else(|| {
            unreachable!("engage_on_arrival: enemy {enemy_id:?} has no location; state corruption")
        });
    let prey = cx.state.enemies[&enemy_id].prey;
    let candidates = cursor::active_investigators_at(cx.state, loc);
    match resolve_prey(cx.state, prey, &candidates) {
        PreyResolution::None => None,
        PreyResolution::One(target) => {
            engage_enemy_with(cx, enemy_id, target);
            None
        }
        PreyResolution::Tie(v) => Some(HunterChoice::Engage {
            enemy: enemy_id,
            candidates: v,
        }),
    }
}

/// Engage a now-unengaged enemy with a co-located investigator per the
/// general engagement rule (Rules Reference p.10): "Any time a ready
/// unengaged enemy is at the same location as an investigator, it
/// engages that investigator … follow the enemy's prey instructions."
///
/// No-op when the enemy is exhausted (an exhausted unengaged enemy does
/// not engage until readied) or has no location. On a prey `Tie` this
/// engages the lead (`tied[0]`, which is `turn_order`-first because
/// `active_investigators_at` is turn-order-ordered) rather than
/// suspending for the lead's `PickSingle` — keeping every defeat
/// caller synchronous. TODO(#151): make the multiplayer tie an
/// interactive lead choice when multiplayer lands.
///
/// Shared primitive: the elimination flow's step-3 re-engagement is the
/// first consumer; Upkeep-4.3 "engage on ready" (#150) will reuse it.
///
/// Precondition: `enemy.engaged_with` must be `None` on entry. This
/// helper engages unconditionally on a `One`/`Tie` resolution and does
/// not disengage a prior target or emit [`Event::EnemyDisengaged`];
/// callers are responsible for clearing (and announcing) any existing
/// engagement first.
pub(super) fn reengage_at_location(cx: &mut Cx, enemy_id: EnemyId) {
    let enemy = &cx.state.enemies[&enemy_id];
    if enemy.exhausted {
        return;
    }
    let Some(loc) = enemy.current_location else {
        return;
    };
    let prey = enemy.prey;
    let candidates = cursor::active_investigators_at(cx.state, loc);
    match resolve_prey(cx.state, prey, &candidates) {
        PreyResolution::None => {}
        PreyResolution::One(target) => engage_enemy_with(cx, enemy_id, target),
        PreyResolution::Tie(tied) => engage_enemy_with(cx, enemy_id, tied[0]),
    }
}

/// Process a single hunter (movement + engage-on-arrival). Returns
/// `Some(HunterChoice)` if a tie suspends, else `None` (fully resolved).
fn process_one_hunter(cx: &mut Cx, enemy_id: EnemyId) -> Option<HunterChoice> {
    let from = cx.state.enemies[&enemy_id]
        .current_location
        .unwrap_or_else(|| {
            unreachable!("process_one_hunter: enemy {enemy_id:?} has no location; state corruption")
        });
    let here = cursor::active_investigators_at(cx.state, from);
    if here.is_empty() {
        let prey = cx.state.enemies[&enemy_id].prey;
        let dests = hunter_destinations(cx.state, from, prey, &cx.state.enemies[&enemy_id]);
        match dests.as_slice() {
            [] => return None,
            [one] => place_enemy_at(cx, enemy_id, *one),
            _ => {
                return Some(HunterChoice::Move {
                    enemy: enemy_id,
                    candidates: dests,
                })
            }
        }
    }
    engage_on_arrival(cx, enemy_id)
}

/// Find the next eligible hunter with id strictly greater than `after`
/// (or the first eligible if `after` is `None`). Scans in ascending
/// `EnemyId` order (`BTreeMap` iteration order).
fn next_eligible_hunter(state: &GameState, after: Option<EnemyId>) -> Option<EnemyId> {
    state
        .enemies
        .iter()
        .filter(|(id, e)| after.is_none_or(|a| **id > a) && is_eligible_hunter(e))
        .map(|(id, _)| *id)
        .next()
}

/// Drive Enemy-phase step 3.2: process eligible hunters in ascending
/// `EnemyId` order until none remain ([`EngineOutcome::Done`]) or one
/// suspends on a lead-investigator tie
/// ([`EngineOutcome::AwaitingInput`]).
pub(crate) fn drive_hunter_moves(cx: &mut Cx) -> EngineOutcome {
    let mut cursor: Option<EnemyId> = None;
    while let Some(id) = next_eligible_hunter(cx.state, cursor) {
        if let Some(choice) = process_one_hunter(cx, id) {
            return suspend_hunter_choice(cx, choice);
        }
        cursor = Some(id);
    }
    EngineOutcome::Done
}

/// Store the pending hunter choice and return `AwaitingInput` for the lead
/// investigator: the candidates ride the request as structured options, and the
/// resume comes back as `PickSingle(OptionId)` indexing the candidate list (#348).
fn suspend_hunter_choice(cx: &mut Cx, choice: HunterChoice) -> EngineOutcome {
    let (prompt, options) = match &choice {
        HunterChoice::Move { enemy, candidates } => (
            format!(
                "Hunter {enemy:?} movement: lead investigator picks a destination among \
                 {candidates:?}"
            ),
            choice::candidate_options(cx.state, candidates, |l| OptionTarget::Location(*l)),
        ),
        HunterChoice::Engage { enemy, candidates } => (
            format!(
                "Hunter {enemy:?} engagement: lead investigator picks whom to engage among \
                 {candidates:?}"
            ),
            choice::candidate_options(cx.state, candidates, |i| {
                choice::investigator_anchor(cx.state, *i)
            }),
        ),
    };
    cx.state
        .continuations
        .push(Continuation::HunterMove(choice));
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(prompt, options),
        resume_token: ResumeToken(0),
    }
}

/// Resume a suspended Hunter-movement choice with the lead
/// investigator's response, then continue driving remaining hunters.
/// Validates the response against the stored candidate set; on an
/// invalid pick, rejects and leaves the `HunterMove` frame on the stack so
/// the client can retry. (#128)
pub(super) fn resume_hunter_choice(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let pending = cx.state.continuations.top_expect::<HunterChoice>().clone();
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: hunter choice expects InputResponse::PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let i = *i as usize;
    let current_enemy = match &pending {
        HunterChoice::Move { enemy, candidates } => {
            let Some(&loc) = candidates.get(i) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter move option {i} out of range (0..{})",
                        candidates.len()
                    )
                    .into(),
                };
            };
            cx.state.continuations.pop_expect::<HunterChoice>();
            place_enemy_at(cx, *enemy, loc);
            // After the move, attempt engage-on-arrival; that itself may
            // suspend on an engagement tie.
            if let Some(choice) = engage_on_arrival(cx, *enemy) {
                return suspend_hunter_choice(cx, choice);
            }
            *enemy
        }
        HunterChoice::Engage { enemy, candidates } => {
            let Some(&who) = candidates.get(i) else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter engage option {i} out of range (0..{})",
                        candidates.len()
                    )
                    .into(),
                };
            };
            cx.state.continuations.pop_expect::<HunterChoice>();
            engage_enemy_with(cx, *enemy, who);
            *enemy
        }
    };
    // Continue with the next eligible hunter after the one we finished.
    let mut cursor = Some(current_enemy);
    while let Some(id) = next_eligible_hunter(cx.state, cursor) {
        if let Some(choice) = process_one_hunter(cx, id) {
            return suspend_hunter_choice(cx, choice);
        }
        cursor = Some(id);
    }
    // All hunters processed (step 3.2 complete) — begin the
    // per-investigator attack loop (step 3.3). Reached only on the
    // no-further-suspension path; every suspension above early-returns
    // via `suspend_hunter_choice`.
    phases::enemy_attack_kickoff(cx)
}

/// Resume a suspended engagement-on-spawn choice (#128, option A) with
/// the lead investigator's `PickSingle`: pop the `SpawnEngage` frame and engage
/// the chosen investigator, then return [`EngineOutcome::Done`].
///
/// Validate-first: an invalid pick (wrong response shape, or a target
/// outside the stored candidate set) rejects and leaves the `SpawnEngage` frame
/// on the stack so the client can retry.
///
/// Continuing the Mythos surge chain is no longer this function's job
/// (callsite-migration). The enemy's `EncounterCard` frame was already popped
/// before `spawn_enemy` suspended (the disposal pops, then spawns — #380), so
/// the `SpawnEngage` frame sits *above* the drawing investigator's
/// [`PlayerDraw`](crate::state::Continuation::PlayerDraw) chain frame. Once we
/// pop the `SpawnEngage` here, that `PlayerDraw` frame is exposed and the
/// `drive` loop's `PlayerDraw` arm continues the chain off its own
/// `surge_pending` (set when the enemy card was drawn). The standalone
/// `EncounterCardRevealed` / agenda-reverse-draw paths have no `PlayerDraw`
/// frame beneath, so the loop simply finishes.
pub(super) fn resume_spawn_engage(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let pending = cx
        .state
        .continuations
        .top_expect::<SpawnEngagePending>()
        .clone();
    let InputResponse::PickSingle(OptionId(i)) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engagement expects InputResponse::PickSingle, got {response:?}"
            )
            .into(),
        };
    };
    let Some(&who) = pending.candidates.get(*i as usize) else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engage option {i} out of range (0..{})",
                pending.candidates.len()
            )
            .into(),
        };
    };
    cx.state.continuations.pop_expect::<SpawnEngagePending>();
    engage_enemy_with(cx, pending.enemy, who);
    // The exposed `PlayerDraw` frame (if any) is driven by the `drive` loop;
    // nothing else to do here.
    EngineOutcome::Done
}

#[cfg(test)]
mod tests;
