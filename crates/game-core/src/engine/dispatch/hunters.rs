//! Hunter-movement and prey-resolution helpers (Enemy phase step 3.2).

use crate::card_data::Prey;
use crate::engine::pathfinding::{bfs_distance, shortest_first_steps};
use crate::event::Event;
use crate::state::{Enemy, EnemyId, GameState, HunterChoice, InvestigatorId, LocationId};

use super::cursor;
use crate::engine::outcome::{EngineOutcome, InputRequest, ResumeToken};

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

/// Narrow `candidates` by `prey`. `Default` treats all candidates as
/// equal; `HighestStat` keeps those with the maximum value of the
/// stat. Returns `One` (single best), `Tie` (2+ best — lead decides),
/// or `None` (empty candidate set). Caller supplies the candidate set
/// (equidistant-nearest investigators for movement; co-located
/// investigators for engagement).
pub(super) fn resolve_prey(
    state: &GameState,
    prey: Prey,
    candidates: &[InvestigatorId],
) -> PreyResolution {
    if candidates.is_empty() {
        return PreyResolution::None;
    }
    let best: Vec<InvestigatorId> = match prey {
        Prey::Default => candidates.to_vec(),
        Prey::HighestStat(stat) => {
            let skill = cursor::stat_to_skill_kind(stat);
            let max = candidates
                .iter()
                .filter_map(|id| {
                    state
                        .investigators
                        .get(id)
                        .map(|inv| inv.skills.value(skill))
                })
                .max();
            match max {
                Some(m) => candidates
                    .iter()
                    .copied()
                    .filter(|id| {
                        state
                            .investigators
                            .get(id)
                            .is_some_and(|inv| inv.skills.value(skill) == m)
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
        // `Prey` is #[non_exhaustive]; new variants (Lowest, Most Clues, etc.)
        // must be wired here when their first card consumer lands — an
        // unrecognised variant at runtime is a card-impl bug.
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
/// equidistant-nearest, prey-filtered investigator. Empty when no
/// investigator is reachable. Deterministic order (sorted `LocationId`).
fn hunter_destinations(state: &GameState, from: LocationId, prey: Prey) -> Vec<LocationId> {
    let mut reachable: Vec<(InvestigatorId, u32)> = Vec::new();
    let mut min_dist: Option<u32> = None;
    for id in &state.turn_order {
        let Some(inv) = state.investigators.get(id) else {
            continue;
        };
        if inv.status != crate::state::Status::Active {
            continue;
        }
        let Some(loc) = inv.current_location else {
            continue;
        };
        let Some(d) = bfs_distance(state, from, loc) else {
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
        for step in shortest_first_steps(state, from, loc) {
            if !dests.contains(&step) {
                dests.push(step);
            }
        }
    }
    dests.sort();
    dests
}

/// Move `enemy` to `to`, emitting [`Event::EnemyMoved`]. Caller has
/// already validated that `to` is a legal destination.
fn move_hunter_to(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
    to: LocationId,
) {
    let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("move_hunter_to: enemy {enemy_id:?} vanished mid-movement; state corruption")
    });
    enemy.current_location = Some(to);
    events.push(Event::EnemyMoved {
        enemy: enemy_id,
        to,
    });
}

/// Set engagement on `enemy_id` → `target` and emit
/// [`Event::EnemyEngaged`]. Shared by movement and spawn.
pub(super) fn engage_enemy_with(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
    target: InvestigatorId,
) {
    let enemy = state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!("engage_enemy_with: enemy {enemy_id:?} vanished; state corruption")
    });
    enemy.engaged_with = Some(target);
    events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator: target,
    });
}

/// Engage-on-arrival for a hunter now at its (possibly unchanged)
/// location. Returns `Some(HunterChoice::Engage{..})` if the co-located
/// set ties under prey (caller suspends); otherwise engages the resolved
/// investigator (or no-one) and returns `None`.
fn engage_on_arrival(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
) -> Option<HunterChoice> {
    let loc = state.enemies[&enemy_id]
        .current_location
        .unwrap_or_else(|| {
            unreachable!("engage_on_arrival: enemy {enemy_id:?} has no location; state corruption")
        });
    let prey = state.enemies[&enemy_id].prey;
    let candidates = cursor::active_investigators_at(state, loc);
    match resolve_prey(state, prey, &candidates) {
        PreyResolution::None => None,
        PreyResolution::One(target) => {
            engage_enemy_with(state, events, enemy_id, target);
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
/// suspending for the lead's `PickInvestigator` — keeping every defeat
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
pub(super) fn reengage_at_location(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
) {
    let enemy = &state.enemies[&enemy_id];
    if enemy.exhausted {
        return;
    }
    let Some(loc) = enemy.current_location else {
        return;
    };
    let prey = enemy.prey;
    let candidates = cursor::active_investigators_at(state, loc);
    match resolve_prey(state, prey, &candidates) {
        PreyResolution::None => {}
        PreyResolution::One(target) => engage_enemy_with(state, events, enemy_id, target),
        PreyResolution::Tie(tied) => engage_enemy_with(state, events, enemy_id, tied[0]),
    }
}

/// Process a single hunter (movement + engage-on-arrival). Returns
/// `Some(HunterChoice)` if a tie suspends, else `None` (fully resolved).
fn process_one_hunter(
    state: &mut GameState,
    events: &mut Vec<Event>,
    enemy_id: EnemyId,
) -> Option<HunterChoice> {
    let from = state.enemies[&enemy_id]
        .current_location
        .unwrap_or_else(|| {
            unreachable!("process_one_hunter: enemy {enemy_id:?} has no location; state corruption")
        });
    let here = cursor::active_investigators_at(state, from);
    if here.is_empty() {
        let prey = state.enemies[&enemy_id].prey;
        let dests = hunter_destinations(state, from, prey);
        match dests.as_slice() {
            [] => return None,
            [one] => move_hunter_to(state, events, enemy_id, *one),
            _ => {
                return Some(HunterChoice::Move {
                    enemy: enemy_id,
                    candidates: dests,
                })
            }
        }
    }
    engage_on_arrival(state, events, enemy_id)
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
pub(crate) fn drive_hunter_moves(state: &mut GameState, events: &mut Vec<Event>) -> EngineOutcome {
    let mut cursor: Option<EnemyId> = None;
    while let Some(id) = next_eligible_hunter(state, cursor) {
        if let Some(choice) = process_one_hunter(state, events, id) {
            return suspend_hunter_choice(state, choice);
        }
        cursor = Some(id);
    }
    EngineOutcome::Done
}

/// Store the pending hunter choice and return `AwaitingInput` for the
/// lead investigator (#128, Task 7 wires the resume path).
fn suspend_hunter_choice(state: &mut GameState, choice: HunterChoice) -> EngineOutcome {
    let prompt = match &choice {
        HunterChoice::Move { enemy, candidates } => format!(
            "Hunter {enemy:?} movement: lead investigator picks a destination among \
             {candidates:?} (submit InputResponse::PickLocation)"
        ),
        HunterChoice::Engage { enemy, candidates } => format!(
            "Hunter {enemy:?} engagement: lead investigator picks whom to engage among \
             {candidates:?} (submit InputResponse::PickInvestigator)"
        ),
    };
    state.hunter_move_pending = Some(choice);
    EngineOutcome::AwaitingInput {
        request: InputRequest { prompt },
        resume_token: ResumeToken(0),
    }
}

/// Resume a suspended Hunter-movement choice with the lead
/// investigator's response, then continue driving remaining hunters.
/// Validates the response against the stored candidate set; on an
/// invalid pick, rejects and leaves `hunter_move_pending` untouched so
/// the client can retry. (#128)
pub(super) fn resume_hunter_choice(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let pending = state.hunter_move_pending.clone().unwrap_or_else(|| {
        unreachable!("resume_hunter_choice: called with no pending hunter choice")
    });
    let current_enemy = match (&pending, response) {
        (
            HunterChoice::Move { enemy, candidates },
            crate::action::InputResponse::PickLocation(loc),
        ) => {
            if !candidates.contains(loc) {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter move destination {loc:?} not among candidates {candidates:?}"
                    )
                    .into(),
                };
            }
            state.hunter_move_pending = None;
            move_hunter_to(state, events, *enemy, *loc);
            // After the move, attempt engage-on-arrival; that itself may
            // suspend on an engagement tie.
            if let Some(choice) = engage_on_arrival(state, events, *enemy) {
                return suspend_hunter_choice(state, choice);
            }
            *enemy
        }
        (
            HunterChoice::Engage { enemy, candidates },
            crate::action::InputResponse::PickInvestigator(who),
        ) => {
            if !candidates.contains(who) {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "ResolveInput: hunter engage target {who:?} not among candidates {candidates:?}"
                    )
                    .into(),
                };
            }
            state.hunter_move_pending = None;
            engage_enemy_with(state, events, *enemy, *who);
            *enemy
        }
        (HunterChoice::Move { .. }, other) => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput: hunter movement expects InputResponse::PickLocation, got {other:?}"
                )
                .into(),
            };
        }
        (HunterChoice::Engage { .. }, other) => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "ResolveInput: hunter engagement expects InputResponse::PickInvestigator, got {other:?}"
                )
                .into(),
            };
        }
    };
    // Continue with the next eligible hunter after the one we finished.
    let mut cursor = Some(current_enemy);
    while let Some(id) = next_eligible_hunter(state, cursor) {
        if let Some(choice) = process_one_hunter(state, events, id) {
            return suspend_hunter_choice(state, choice);
        }
        cursor = Some(id);
    }
    // All hunters processed (step 3.2 complete) — begin the
    // per-investigator attack loop (step 3.3). Reached only on the
    // no-further-suspension path; every suspension above early-returns
    // via `suspend_hunter_choice`.
    super::phases::enemy_attack_kickoff(state, events);
    EngineOutcome::Done
}

/// Resume a suspended engagement-on-spawn choice (#128, option A) with
/// the lead investigator's `PickInvestigator`, then continue the drawing
/// investigator's Mythos encounter-draw chain.
///
/// Validate-first: an invalid pick (wrong response shape, or a target
/// outside the stored candidate set) rejects and leaves
/// `spawn_engage_pending` untouched so the client can retry.
///
/// The chain only resumes when the suspension arose mid-Mythos-draw —
/// i.e. the drawing investigator is still the pending cursor. The
/// single-draw `EncounterCardRevealed` path (`mythos_draw_pending` is
/// `None`, or points elsewhere) engages and stops at `Done` without
/// touching the cursor.
pub(super) fn resume_spawn_engage(
    state: &mut GameState,
    events: &mut Vec<Event>,
    response: &crate::action::InputResponse,
) -> EngineOutcome {
    let pending = state.spawn_engage_pending.clone().unwrap_or_else(|| {
        unreachable!("resume_spawn_engage: called with no pending spawn engagement")
    });
    let crate::action::InputResponse::PickInvestigator(who) = response else {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engagement expects InputResponse::PickInvestigator, \
                 got {response:?}"
            )
            .into(),
        };
    };
    if !pending.candidates.contains(who) {
        return EngineOutcome::Rejected {
            reason: format!(
                "ResolveInput: spawn engage target {who:?} not among candidates {:?}",
                pending.candidates
            )
            .into(),
        };
    }
    state.spawn_engage_pending = None;
    engage_enemy_with(state, events, pending.enemy, *who);

    // Only re-enter the Mythos surge chain if the suspend happened
    // mid-chain (the drawing investigator is still the pending cursor).
    // The `EncounterCardRevealed` single-draw path resolves to `Done`.
    //
    // Invariant: while `spawn_engage_pending` is set, the apply guard
    // (line 129) rejects every non-`ResolveInput` action, so nothing can
    // retarget the Mythos cursor between suspend and resume. Hence
    // `mythos_draw_pending == Some(investigator_to_draw)` reliably means
    // "we suspended mid-chain for this investigator."
    if state.mythos_draw_pending == Some(pending.investigator_to_draw) {
        super::encounter::run_mythos_draw_chain(
            state,
            events,
            pending.investigator_to_draw,
            pending.chain_count,
            pending.surge,
        )
    } else {
        EngineOutcome::Done
    }
}

#[cfg(test)]
mod resolve_prey_tests {
    use super::*;
    use crate::test_support::{test_investigator, TestGame};

    #[test]
    fn resolve_prey_default_single_candidate_is_one() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::Default,
            &[InvestigatorId(1)],
        );
        assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
    }

    #[test]
    fn resolve_prey_default_multiple_is_tie() {
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::Default,
            &[InvestigatorId(1), InvestigatorId(2)],
        );
        assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
    }

    #[test]
    fn resolve_prey_empty_is_none() {
        let state = TestGame::new().build();
        let r = resolve_prey(&state, crate::card_data::Prey::Default, &[]);
        assert!(matches!(r, PreyResolution::None));
    }

    #[test]
    fn resolve_prey_highest_stat_picks_max() {
        let mut hi = test_investigator(1);
        hi.skills.combat = 5;
        let mut lo = test_investigator(2);
        lo.skills.combat = 2;
        let state = TestGame::new()
            .with_investigator(hi)
            .with_investigator(lo)
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat),
            &[InvestigatorId(1), InvestigatorId(2)],
        );
        assert!(matches!(r, PreyResolution::One(id) if id == InvestigatorId(1)));
    }

    #[test]
    fn resolve_prey_highest_stat_tie_is_tie() {
        let mut a = test_investigator(1);
        a.skills.combat = 4;
        let mut b = test_investigator(2);
        b.skills.combat = 4;
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .build();
        let r = resolve_prey(
            &state,
            crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat),
            &[InvestigatorId(1), InvestigatorId(2)],
        );
        assert!(matches!(r, PreyResolution::Tie(ref v) if v.len() == 2));
    }
}

#[cfg(test)]
mod hunter_movement_tests {
    use super::*;
    use crate::state::{EnemyId, InvestigatorId, LocationId, Phase};
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};
    use crate::{assert_event, assert_no_event};

    #[test]
    fn hunter_moves_one_step_toward_investigator_two_hops_away_no_engage() {
        // Map: A(1)-B(2)-C(3). Investigator at C; hunter at A. Hunter moves
        // A->B (one step). No investigator at B, so no engage yet.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut c = test_location(3, "C");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1), LocationId(3)];
        c.connections = vec![LocationId(2)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(3));
        let mut ghoul = test_enemy(1, "Swarm");
        ghoul.hunter = true;
        ghoul.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_location(c)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(ghoul)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
        assert_event!(events, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(2));
    }

    #[test]
    fn hunter_engages_when_it_moves_into_investigators_location() {
        // Map A(1)-B(2). Investigator at B; hunter at A. Hunter moves A->B
        // and engages on arrival.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(h)
            .build();
        let mut events = Vec::new();
        drive_hunter_moves(&mut state, &mut events);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(InvestigatorId(1))
        );
        assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
    }

    #[test]
    fn hunter_with_no_path_does_not_move() {
        let mut a = test_location(1, "A");
        let island = test_location(9, "Island");
        a.connections = vec![];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(1));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.current_location = Some(LocationId(9));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(island)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(h)
            .build();
        let mut events = Vec::new();
        drive_hunter_moves(&mut state, &mut events);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(9))
        );
        assert_no_event!(events, Event::EnemyMoved { .. });
    }

    #[test]
    fn exhausted_hunter_is_skipped() {
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.exhausted = true;
        h.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(h)
            .build();
        let mut events = Vec::new();
        drive_hunter_moves(&mut state, &mut events);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(1))
        );
        assert_no_event!(events, Event::EnemyMoved { .. });
    }

    #[test]
    fn non_hunter_enemy_does_not_move() {
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(2));
        let mut e = test_enemy(1, "Slug");
        e.hunter = false;
        e.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(e)
            .build();
        let mut events = Vec::new();
        drive_hunter_moves(&mut state, &mut events);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(1))
        );
        assert_no_event!(events, Event::EnemyMoved { .. });
    }

    #[test]
    fn hunter_already_co_located_does_not_move_but_engages() {
        // Hunter and investigator both at A(1). p.12: an enemy already at a
        // location with an investigator does not move; it still engages.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(1));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(h)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(1))
        );
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(InvestigatorId(1))
        );
        assert_no_event!(events, Event::EnemyMoved { .. });
        assert_event!(events, Event::EnemyEngaged { enemy, investigator } if *enemy == EnemyId(1) && *investigator == InvestigatorId(1));
    }
}

#[cfg(test)]
mod hunter_resume_tests {
    use super::*;
    use crate::assert_event;
    use crate::state::{EnemyId, InvestigatorId, LocationId, Phase};
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};

    #[test]
    fn hunter_move_tie_suspends_then_resumes_on_pick_location() {
        // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
        // default prey. Two equal first-steps (B, C) -> AwaitingInput.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(state.hunter_move_pending.is_some());
        // Resume by picking C.
        let mut ev2 = Vec::new();
        let resumed = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickLocation(LocationId(3)),
        );
        assert_eq!(resumed, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(3))
        );
        assert!(state.hunter_move_pending.is_none());
        assert_event!(ev2, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(3));
    }

    #[test]
    fn hunter_move_tie_rejects_invalid_pick() {
        // Same diamond setup; resume with a location not in candidates.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        drive_hunter_moves(&mut state, &mut events);
        let mut ev2 = Vec::new();
        // LocationId(4) is the destination, not a first-step candidate.
        let result = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickLocation(LocationId(4)),
        );
        assert!(matches!(result, EngineOutcome::Rejected { .. }));
        assert!(
            state.hunter_move_pending.is_some(),
            "pending stays open on invalid pick"
        );
    }

    #[test]
    fn hunter_engage_tie_suspends_then_resumes_on_pick_investigator() {
        // Two investigators at B; hunter moves A->B; default prey -> tie ->
        // PickInvestigator.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1)];
        let mut i1 = test_investigator(1);
        i1.current_location = Some(LocationId(2));
        let mut i2 = test_investigator(2);
        i2.current_location = Some(LocationId(2));
        let mut h = test_enemy(1, "Hunter");
        h.hunter = true;
        h.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_investigator(i1)
            .with_investigator(i2)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .with_enemy(h)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        // Moved to B already, suspended on engagement tie.
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        let mut ev2 = Vec::new();
        let resumed = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickInvestigator(InvestigatorId(2)),
        );
        assert_eq!(resumed, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(InvestigatorId(2))
        );
        assert!(state.hunter_move_pending.is_none());
    }

    #[test]
    fn highest_combat_prey_breaks_move_tie_without_prompt() {
        // Fan A(1)-{B(2),C(3)}. inv1 at B combat 5; inv2 at C combat 2.
        // hunter at A with HighestStat(Combat) prey. resolve_prey picks
        // inv1 unambiguously -> moves A->B, engages, no prompt.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1)];
        loc_c.connections = vec![LocationId(1)];
        let mut inv1 = test_investigator(1);
        inv1.current_location = Some(LocationId(2));
        inv1.skills.combat = 5;
        let mut inv2 = test_investigator(2);
        inv2.current_location = Some(LocationId(3));
        inv2.skills.combat = 2;
        let mut hunter = test_enemy(1, "Ghoul Priest");
        hunter.hunter = true;
        hunter.prey = crate::card_data::Prey::HighestStat(crate::dsl::Stat::Combat);
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_investigator(inv1)
            .with_investigator(inv2)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert_eq!(outcome, EngineOutcome::Done);
        // Moves toward inv1 (B) and engages immediately (arrives at B).
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(InvestigatorId(1))
        );
    }

    #[test]
    fn multi_hunter_one_suspends_then_next_processed_on_resume() {
        // Diamond A(1)-{B(2),C(3)}-D(4). inv at D(4).
        // Hunter1 at A(1) ties B/C; hunter2 at B(2) has clean B->D step.
        // drive suspends on hunter1; resume picks B; then hunter2
        // processes automatically: moves B->D and engages.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut tie_hunter = test_enemy(1, "Tie Hunter");
        tie_hunter.hunter = true;
        tie_hunter.current_location = Some(LocationId(1)); // ties B/C toward D
        let mut clean_hunter = test_enemy(2, "Clean Hunter");
        clean_hunter.hunter = true;
        clean_hunter.current_location = Some(LocationId(2)); // single step B->D
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(tie_hunter)
            .with_enemy(clean_hunter)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        // Resolve hunter 1's tie -> hunter 2 then moves B->D and engages.
        let mut ev2 = Vec::new();
        let resumed = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickLocation(LocationId(2)),
        );
        assert_eq!(resumed, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(2)].current_location,
            Some(LocationId(4))
        );
        assert_eq!(
            state.enemies[&EnemyId(2)].engaged_with,
            Some(InvestigatorId(1))
        );
    }

    #[test]
    fn hunter_move_tie_rejects_wrong_response_kind() {
        // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
        // default prey. Two equal first-steps (B, C) -> AwaitingInput on Move.
        // Client submits PickInvestigator instead of PickLocation -> Rejected,
        // pending preserved for retry.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(state.hunter_move_pending.is_some());
        // Submit PickInvestigator when PickLocation is expected.
        let mut ev2 = Vec::new();
        let result = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickInvestigator(InvestigatorId(1)),
        );
        assert!(
            matches!(result, EngineOutcome::Rejected { .. }),
            "wrong response kind should be rejected"
        );
        assert!(
            state.hunter_move_pending.is_some(),
            "pending preserved so client can retry with PickLocation"
        );
    }

    #[test]
    fn hunter_engage_tie_rejects_wrong_response_kind() {
        // Two investigators at B(2); hunter moves A(1)->B(2); default prey
        // -> engage tie -> AwaitingInput on Engage.
        // Client submits PickLocation instead of PickInvestigator -> Rejected,
        // pending preserved for retry.
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        loc_a.connections = vec![LocationId(2)];
        loc_b.connections = vec![LocationId(1)];
        let mut inv1 = test_investigator(1);
        inv1.current_location = Some(LocationId(2));
        let mut inv2 = test_investigator(2);
        inv2.current_location = Some(LocationId(2));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        let mut state = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_investigator(inv1)
            .with_investigator(inv2)
            .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
            .with_enemy(hunter)
            .build();
        let mut events = Vec::new();
        let outcome = drive_hunter_moves(&mut state, &mut events);
        // Moved to B already, suspended on engagement tie.
        assert_eq!(
            state.enemies[&EnemyId(1)].current_location,
            Some(LocationId(2))
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert!(state.hunter_move_pending.is_some());
        // Submit PickLocation when PickInvestigator is expected.
        let mut ev2 = Vec::new();
        let result = super::super::resolve_input(
            &mut state,
            &mut ev2,
            &crate::action::InputResponse::PickLocation(LocationId(1)),
        );
        assert!(
            matches!(result, EngineOutcome::Rejected { .. }),
            "wrong response kind should be rejected"
        );
        assert!(
            state.hunter_move_pending.is_some(),
            "pending preserved so client can retry with PickInvestigator"
        );
    }
}

#[cfg(test)]
mod reengage_tests {
    use super::*;
    use crate::assert_event;
    use crate::assert_no_event;
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};

    #[test]
    fn reengage_at_location_engages_sole_co_located_survivor() {
        let surv = InvestigatorId(2);
        let loc = LocationId(1);
        let survivor = {
            let mut i = test_investigator(2);
            i.current_location = Some(loc);
            i
        };
        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = None;
            e
        };
        let mut state = TestGame::default()
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([surv])
            .build();
        let mut events = Vec::new();

        reengage_at_location(&mut state, &mut events, EnemyId(1));

        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(surv));
        assert_event!(events, Event::EnemyEngaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == surv);
    }

    #[test]
    fn reengage_at_location_no_co_located_investigator_leaves_unengaged() {
        let loc = LocationId(1);
        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = None;
            e
        };
        let mut state = TestGame::default()
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([])
            .build();
        let mut events = Vec::new();

        reengage_at_location(&mut state, &mut events, EnemyId(1));

        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn reengage_at_location_tie_auto_picks_lead_first_in_turn_order() {
        // Two co-located survivors, Prey::Default → tie → engage turn_order-first (lead).
        let lead = InvestigatorId(2);
        let other = InvestigatorId(3);
        let loc = LocationId(1);
        let mk = |raw: u32| {
            let mut i = test_investigator(raw);
            i.current_location = Some(loc);
            i
        };
        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = None;
            e.prey = crate::card_data::Prey::Default;
            e
        };
        let mut state = TestGame::default()
            .with_investigator(mk(2))
            .with_investigator(mk(3))
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([lead, other]) // lead first
            .build();
        let mut events = Vec::new();

        reengage_at_location(&mut state, &mut events, EnemyId(1));

        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(lead),
            "tie engages the lead (turn_order-first)"
        );
        assert_event!(events, Event::EnemyEngaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == lead);
    }

    #[test]
    fn reengage_at_location_exhausted_enemy_does_not_engage() {
        let surv = InvestigatorId(2);
        let loc = LocationId(1);
        let survivor = {
            let mut i = test_investigator(2);
            i.current_location = Some(loc);
            i
        };
        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = None;
            e.exhausted = true; // exhausted unengaged enemy does not engage (RR p.10)
            e
        };
        let mut state = TestGame::default()
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([surv])
            .build();
        let mut events = Vec::new();

        reengage_at_location(&mut state, &mut events, EnemyId(1));

        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn reengage_at_location_enemy_without_location_is_noop() {
        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = None; // no location — must no-op
            e.engaged_with = None;
            e
        };
        let mut state = TestGame::default().with_enemy(enemy).build();
        let mut events = Vec::new();
        reengage_at_location(&mut state, &mut events, EnemyId(1));
        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }
}
