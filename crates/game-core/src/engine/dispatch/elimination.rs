//! Investigator elimination helpers: defeat application, elimination
//! steps, horror application, and all-defeated detection.

use crate::event::Event;
use crate::state::{DefeatCause, EnemyId, GameState, InvestigatorId, Status};

#[cfg(test)]
use crate::state::{CardCode, CardInPlay, CardInstanceId, LocationId, Phase};

/// Flip an Active investigator's status to the appropriate defeated
/// variant for `cause`, emit [`Event::InvestigatorDefeated`], and run
/// [`check_all_defeated`]. No-op if the investigator is already
/// non-Active (an investigator can only be defeated once per attack).
///
/// [`Status::Killed`]: crate::state::Status::Killed
/// [`Status::Insane`]: crate::state::Status::Insane
pub(super) fn apply_investigator_defeat(
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

    // Rules Reference p.10 Elimination steps 1–5 run here, between the
    // defeat event and the all-defeated check (step 6 signal). See the
    // design doc 2026-05-31-144 for the full breakdown.
    run_elimination_steps(state, events, investigator);

    check_all_defeated(state, events);
}

/// Execute Rules Reference p.10 Elimination steps 1–5 for an
/// investigator whose `status` has just been flipped to a defeated
/// variant. Synchronous: the step-3 re-engagement tie auto-picks the
/// lead rather than suspending (see `reengage_at_location`).
fn run_elimination_steps(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
) {
    // The location the investigator was at "when eliminated" — read once
    // before any mutations; step 2 deposits clues here.
    let last_location = state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location);

    // Step 1: remove every card this investigator controls in play and
    // owns in out-of-play areas (hand/deck/discard) from the game.
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
            )
        });
    // Build the pile in an owned local so each mutation borrows only one
    // field of `inv` at a time (mutating `inv.removed_from_game` directly
    // while borrowing `inv.hand` etc. would double-borrow `inv` — rejected
    // by the borrow checker).
    let mut removed = std::mem::take(&mut inv.removed_from_game);
    removed.extend(inv.cards_in_play.drain(..).map(|c| c.code));
    removed.append(&mut inv.hand);
    removed.append(&mut inv.deck);
    removed.append(&mut inv.discard);
    inv.removed_from_game = removed;

    // Step 2: place possessed clues at the location; return resources to
    // the (unmodeled, infinite) token pool by zeroing them.
    let clues = inv.clues;
    inv.clues = 0;
    inv.resources = 0;
    if clues > 0 {
        if let Some(loc_id) = last_location {
            if let Some(loc) = state.locations.get_mut(&loc_id) {
                loc.clues = loc.clues.saturating_add(clues);
                let new_count = loc.clues;
                events.push(Event::LocationCluesChanged {
                    location: loc_id,
                    new_count,
                });
            }
        }
    }

    // Step 3: disengage every enemy engaged with the eliminated
    // investigator, leaving them "at the location the investigator was
    // at when eliminated, unengaged but otherwise maintaining their
    // current game state" (RR p.10). Engaged enemies already share the
    // investigator's location by the engagement invariant (Move drags
    // them along), so no location update is needed — just clear
    // `engaged_with`. Disengage all first (simultaneous), then let the
    // ready ones re-engage a surviving co-located investigator per prey.
    let affected: Vec<EnemyId> = state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    for &eid in &affected {
        let enemy = state.enemies.get_mut(&eid).unwrap_or_else(|| {
            unreachable!("run_elimination_steps: enemy {eid:?} vanished; state corruption")
        });
        enemy.engaged_with = None;
        events.push(Event::EnemyDisengaged {
            enemy: eid,
            investigator,
        });
    }
    for &eid in &affected {
        super::hunters::reengage_at_location(state, events, eid);
    }

    // Step 4: place other (non-enemy) threat-area cards in the
    // appropriate discard pile. No-op: treachery/asset-in-threat-area
    // state is not modeled yet (enemies are the only threat-area
    // occupants). TODO: wire when threat-area cards land (Phase 7+).

    // Step 5: lead-investigator transfer. No-op by construction: there
    // is no stored lead; `first_active_investigator` recomputes the lead
    // as the first Active investigator in `turn_order`, so a defeated
    // lead is automatically replaced. UX for "remaining players choose"
    // is deferred (Phase 8, #151) alongside the re-engagement-tie pick.

    // Step 6 (no remaining players => scenario ends) is signaled by
    // `check_all_defeated` (caller) emitting AllInvestigatorsDefeated
    // and latching Resolution::Lost; the `apply` hook turns that latch
    // into ScenarioResolved + apply_resolution.

    // The investigator has left play — clear their location last, after
    // step 2 deposited clues using `last_location` (step 3 reads
    // `enemy.current_location` directly, relying on the same value via
    // the engagement invariant).
    let inv = state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
            )
        });
    inv.current_location = None;
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
/// with simultaneous-placement semantics (i.e. [`enemy_attack`](super::combat::enemy_attack) and
/// any future card effect that deals both) compose the lower-level
/// [`apply_damage_numeric`](super::combat::apply_damage_numeric) + [`apply_horror_numeric`](super::combat::apply_horror_numeric) +
/// [`apply_investigator_defeat`] triple instead. A `take_damage`
/// twin is not provided because no single-source-damage caller exists
/// yet; the recipe (numeric helper + defeat application on `true`
/// return) is one line per call site.
///
/// [`Status::Insane`]: crate::state::Status::Insane
pub(super) fn take_horror(
    state: &mut GameState,
    events: &mut Vec<Event>,
    investigator: InvestigatorId,
    amount: u8,
) {
    if super::combat::apply_horror_numeric(state, events, investigator, amount) {
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
/// Idempotent on subsequent defeats: the predicate becomes true at the
/// first all-defeated transition and stays true. Callers only invoke it
/// after a status flip, so the event fires exactly once per scenario in
/// practice; the resolution latch is likewise transition-bounded
/// (first-writer-wins).
///
/// Mutates `state` via the resolution latch (below): on the no-active-
/// investigator transition it requests [`crate::scenario::Resolution::Lost`]
/// per Rules Reference p.10 step 6. The `apply` hook turns that latch into
/// [`Event::ScenarioResolved`] + `apply_resolution`.
pub(super) fn check_all_defeated(state: &mut GameState, events: &mut Vec<Event>) {
    let any_active = state
        .investigators
        .values()
        .any(|inv| inv.status == Status::Active);
    // Empty-investigators is nonsense scenario state; suppress the
    // event so we don't emit a meaningless "all defeated" when there
    // was nobody to defeat in the first place.
    if !any_active && !state.investigators.is_empty() {
        events.push(Event::AllInvestigatorsDefeated);
        // Rules Reference p.10 step 6: "If there are no remaining players,
        // the scenario ends. Refer to 'no resolution was reached' entry
        // for that scenario in the campaign guide." Latch the loss
        // (first-writer-wins, so an already-fired act/agenda resolution
        // stays authoritative).
        super::request_resolution(
            state,
            crate::scenario::Resolution::Lost {
                reason: "no resolution was reached".into(),
            },
        );
    }
}

#[cfg(test)]
mod elimination_tests {
    use super::*;
    use crate::assert_event;
    use crate::assert_no_event;
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};

    #[test]
    fn elimination_step1_removes_controlled_and_owned_cards() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.max_health = 1;
        inv.hand = vec![CardCode("h1".into()), CardCode("h2".into())];
        inv.deck = vec![CardCode("d1".into())];
        inv.discard = vec![CardCode("x1".into())];
        inv.cards_in_play = vec![CardInPlay::enter_play(
            CardCode("p1".into()),
            CardInstanceId(1),
        )];

        let mut state = TestGame::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, id, DefeatCause::Damage);

        let after = &state.investigators[&id];
        assert!(after.hand.is_empty(), "hand drained");
        assert!(after.deck.is_empty(), "deck drained");
        assert!(after.discard.is_empty(), "discard drained");
        assert!(after.cards_in_play.is_empty(), "cards_in_play drained");
        // All five codes landed in the removed pile (order: in-play, hand, deck, discard).
        let removed: Vec<&str> = after
            .removed_from_game
            .iter()
            .map(CardCode::as_str)
            .collect();
        assert_eq!(removed.len(), 5, "all controlled/owned cards removed");
        assert!(removed.contains(&"p1"));
        assert!(removed.contains(&"h1"));
        assert!(removed.contains(&"d1"));
        assert!(removed.contains(&"x1"));
    }

    #[test]
    fn elimination_step2_places_clues_at_location_and_zeroes_resources() {
        let id = InvestigatorId(1);
        let loc_id = LocationId(1);
        let mut inv = test_investigator(1);
        inv.max_health = 1;
        inv.current_location = Some(loc_id);
        inv.clues = 2;
        inv.resources = 4;

        let mut loc = test_location(1, "Study");
        loc.clues = 1;

        let mut state = TestGame::default()
            .with_investigator(inv)
            .with_location(loc)
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, id, DefeatCause::Damage);

        assert_eq!(
            state.locations[&loc_id].clues, 3,
            "2 investigator clues added to location's 1"
        );
        assert_eq!(
            state.investigators[&id].clues, 0,
            "investigator clues cleared"
        );
        assert_eq!(
            state.investigators[&id].resources, 0,
            "resources returned to pool"
        );
        assert_event!(events, Event::LocationCluesChanged { location, new_count: 3 } if *location == loc_id);
    }

    #[test]
    fn elimination_step3_disengages_then_reengages_ready_enemy_onto_survivor() {
        let dead = InvestigatorId(1);
        let surv = InvestigatorId(2);
        let loc = LocationId(1);

        let mut dying = test_investigator(1);
        dying.max_health = 1;
        dying.current_location = Some(loc);

        let mut survivor = test_investigator(2);
        survivor.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead); // engaged with the about-to-die investigator
            e
        };

        let mut state = TestGame::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

        assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == dead);
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(surv),
            "ready enemy re-engages the co-located survivor"
        );
        assert_event!(events, Event::EnemyEngaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == surv);
        assert_eq!(state.enemies[&EnemyId(1)].current_location, Some(loc));
        assert_eq!(
            state.investigators[&dead].current_location, None,
            "eliminated => between locations"
        );
    }

    #[test]
    fn elimination_step3_solo_defeat_leaves_enemy_unengaged() {
        let dead = InvestigatorId(1);
        let loc = LocationId(1);

        let mut dying = test_investigator(1);
        dying.max_health = 1;
        dying.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead);
            e
        };

        let mut state = TestGame::default()
            .with_investigator(dying)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

        assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == dead);
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            None,
            "no surviving co-located investigator => stays unengaged"
        );
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn last_investigator_defeated_latches_lost_resolution() {
        // Single investigator; defeat them and assert the no-remaining-players
        // resolution latch is set (Rules Reference p.10 step 6).
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.max_sanity = 1;
        let mut state = TestGame::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        let mut events = Vec::new();

        // Apply lethal horror through the standard defeat path.
        take_horror(&mut state, &mut events, inv, 1);

        assert_event!(events, Event::AllInvestigatorsDefeated);
        assert!(
            matches!(
                state.resolution,
                Some(crate::scenario::Resolution::Lost { .. })
            ),
            "no-remaining-players must latch Lost"
        );
    }

    #[test]
    fn elimination_runs_on_horror_defeat_too() {
        let dead = InvestigatorId(1);
        let surv = InvestigatorId(2);
        let loc = LocationId(1);

        let mut dying = test_investigator(1);
        dying.max_sanity = 1;
        dying.current_location = Some(loc);
        dying.clues = 1;

        let mut survivor = test_investigator(2);
        survivor.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Whippoorwill");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead);
            e
        };

        let mut state = TestGame::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Horror);

        assert_eq!(state.investigators[&dead].status, Status::Insane);
        assert_eq!(state.locations[&loc].clues, 1, "clue placed at location");
        assert_eq!(
            state.enemies[&EnemyId(1)].engaged_with,
            Some(surv),
            "re-engaged survivor"
        );
        assert_eq!(state.investigators[&dead].current_location, None);
    }

    #[test]
    fn elimination_step3_exhausted_engaged_enemy_disengages_but_does_not_reengage() {
        let dead = InvestigatorId(1);
        let surv = InvestigatorId(2);
        let loc = LocationId(1);

        let mut dying = test_investigator(1);
        dying.max_health = 1;
        dying.current_location = Some(loc);

        let mut survivor = test_investigator(2);
        survivor.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead);
            e.exhausted = true; // does not re-engage even with a co-located survivor
            e
        };

        let mut state = TestGame::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, dead, DefeatCause::Damage);

        assert_event!(events, Event::EnemyDisengaged { enemy, investigator }
            if *enemy == EnemyId(1) && *investigator == dead);
        assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
        assert_no_event!(events, Event::EnemyEngaged { .. });
    }

    #[test]
    fn elimination_without_location_skips_clue_placement_and_does_not_panic() {
        // Defeated "between locations" (current_location == None): step 2
        // must skip clue placement (the clues leave play with the
        // investigator) and zero resources without panicking.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.max_health = 1;
        inv.current_location = None;
        inv.clues = 3;
        inv.resources = 2;

        let mut state = TestGame::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(&mut state, &mut events, id, DefeatCause::Damage);

        assert_eq!(
            state.investigators[&id].clues, 0,
            "clues cleared (left play)"
        );
        assert_eq!(state.investigators[&id].resources, 0, "resources returned");
        assert_no_event!(events, Event::LocationCluesChanged { .. });
    }
}
