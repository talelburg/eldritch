//! Investigator elimination helpers: defeat application, elimination
//! steps, horror application, and all-defeated detection.

use super::super::outcome::EngineOutcome;
use super::Cx;
use crate::event::Event;
use crate::state::{
    CardInPlay, CardInstanceId, Continuation, DefeatCause, EliminationStep, EnemyId,
    InvestigatorId, Status,
};

#[cfg(test)]
use crate::state::{CardCode, LocationId, Phase};

/// Flip an Active investigator's status to the appropriate defeated variant for
/// `cause` and emit [`Event::InvestigatorDefeated`]. No-op if the investigator
/// is already non-Active (an investigator can only be defeated once per attack).
///
/// # Then one of two paths (#638)
///
/// - **No step-0 weakness ability** (every elimination but a Roland holding
///   clues on Cover Up): [`run_elimination_steps`] and [`check_all_defeated`]
///   run inline before this returns, as they always have.
/// - **A step-0 weakness ability**: a [`Continuation::Elimination`] frame is
///   pushed and this returns immediately. Steps 1–6 *and*
///   [`check_all_defeated`] run later, from [`drive_elimination`], once the
///   queued abilities have drained — so on this path a caller that resumes
///   after this function sees an elimination still **in progress**: status
///   flipped, but cards not yet removed and no `AllInvestigatorsDefeated` /
///   `ScenarioEnding` latch yet. [`super::combat::place_assignment`] is the
///   only such caller today and gates on [`Status`] for exactly this reason.
///
/// [`Status`]: crate::state::Status
/// [`Status::Killed`]: crate::state::Status::Killed
/// [`Status::Insane`]: crate::state::Status::Insane
pub(super) fn apply_investigator_defeat(
    cx: &mut Cx,
    investigator: InvestigatorId,
    cause: DefeatCause,
) {
    let inv = cx.state
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
        DefeatCause::CardAbility => Status::DefeatedByCardAbility,
    };
    cx.events.push(Event::InvestigatorDefeated {
        investigator,
        cause,
    });

    // If it was their turn, that turn is over (#764).
    end_turn_on_elimination(cx, investigator);

    // Rules Reference p.10 Elimination step 0 (#638). The rule, why the steps
    // have to ride a frame to honour it, and what that costs are all documented
    // once on `Continuation::Elimination`; this is the fork it describes.
    if has_weakness_game_end_ability(cx.state, investigator) {
        cx.state.continuations.push(Continuation::Elimination {
            investigator,
            step: EliminationStep::FireWeaknessGameEnd,
        });
        return;
    }

    // Rules Reference p.10 Elimination steps 1–5 run here, between the
    // defeat event and the all-defeated check (step 6 signal). See the
    // design doc 2026-05-31-144 for the full breakdown.
    run_elimination_steps(cx, investigator);

    check_all_defeated(cx);
}

/// End the active investigator's turn when elimination takes them out of it
/// (#764): announce [`Event::TurnEnded`] and arm their
/// [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame's `ending`
/// flag so the `drive` loop runs the rotation tail
/// ([`resume_end_turn`](super::phases::resume_end_turn)) once every frame above
/// it has unwound. The lookup
/// ([`turn_frame_ending_mut`](super::cursor::turn_frame_ending_mut)) is keyed by
/// investigator, so this is a no-op when it is not their turn: a defeat in the
/// Mythos or Enemy phase finds no turn frame at all, and one dealt to a bystander
/// by Dynamite Blast 01024 leaves the *active* investigator's frame alone rather
/// than ending someone else's turn.
///
/// # The rule
///
/// **Not** the Elimination entry, which says nothing about turns: the basis is
/// Rules Reference Appendix II step 2.2.1, *"If the investigator does not or
/// cannot take an action, proceed to 2.2.2."* An eliminated investigator cannot
/// take one — every action's validation gate rejects a non-`Active` actor — so
/// 2.2.2 is where the turn goes. Step 2.2.2 is then the rotation this arms:
/// *"If there is an investigator who has not yet taken a turn this round, return
/// to 2.2. If each investigator has taken a turn this round, proceed to 2.3."*
///
/// # Why the flag rather than [`super::phases::end_turn`]
///
/// `end_turn` emits the `EndOfTurn` timing point, and this function runs deep
/// inside the defeat sequence (an attack of opportunity's damage placement, a
/// treachery's revelation test) rather than in tail position — so emitting here
/// would queue ability frames beneath everything still unwinding, the ADR 0003
/// defect class. It would also be wrong on its own terms: Elimination step 1
/// removes the investigator's cards from the game, so there is nothing left for
/// an *"at the end of your turn"* ability to hang on.
///
/// Arming the flag is inert by comparison — the frame is already on the stack,
/// and the loop reaches it in its own time. That also makes this safe on the
/// step-0 weakness path, where [`apply_investigator_defeat`] returns before
/// steps 1–6 have run: the frame waits for [`drive_elimination`] to finish
/// either way.
///
/// # `actions_remaining` is deliberately not drained
///
/// [`super::phases::end_turn`] drains it; this does not. The count is the record
/// that the action which killed them was charged — a Move into a lethal attack
/// of opportunity spends its action and *then* has its relocation suppressed,
/// which `move_with_lethal_aoo_suppresses_relocation_but_keeps_spent_action`
/// pins. Zeroing here would erase that. The count is inert either way: an armed
/// frame never enumerates, and every action's validation gate rejects a
/// non-`Active` actor.
///
/// # Where [`Event::TurnEnded`] lands
///
/// Before the elimination steps' own events, and before the step-0 weakness fork
/// returns. That is deliberate: the fork means steps 1–6 run either inline or
/// later from [`drive_elimination`], so announcing here is the one position that
/// fires exactly once on both paths without duplicating the push into the frame
/// driver. The turn is over the moment the status flips, so the log reads
/// `InvestigatorDefeated` → `TurnEnded` → the teardown that follows.
///
/// # All investigators defeated
///
/// The armed frame is never resumed: `check_all_defeated` latches
/// `ScenarioEnding::NoResolution`, and an `InvestigatorTurn` is
/// [cancelled by a latched resolution](Continuation::cancelled_by_scenario_end),
/// so `drive` pops it rather than rotating (ADR 0004). Solo defeat therefore
/// ends the scenario, and this arming is what keeps a *surviving* table moving.
fn end_turn_on_elimination(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(ending) = super::cursor::turn_frame_ending_mut(cx.state, investigator) else {
        return;
    };
    // Already armed: the player submitted `EndTurn` and a suspending `EndOfTurn`
    // forced ability killed them (Frozen in Fear 01164's willpower test). The
    // turn is ending exactly once, and `end_turn` already announced it.
    if *ending {
        return;
    }
    *ending = true;
    cx.events.push(Event::TurnEnded { investigator });
}

/// Whether `investigator` owns an in-play weakness carrying a *"when the game
/// ends"* Forced ability — i.e. whether Elimination step 0 has anything to fire.
///
/// Asks the step-0 scan itself rather than re-deriving the predicate, so the
/// fork in [`apply_investigator_defeat`] cannot drift from what the scan
/// collects — including its RR p.2 "no potential to change the game state" drop,
/// which is conservative for a native effect (a Cover Up holding no clues still
/// routes elimination onto the frame; the ability then resolves to nothing,
/// which is the same observable outcome as never firing).
///
/// **Every cell, not just `after`.** The scan is per-cell, and this predicate
/// asks a question about the whole sequence — so hardcoding one cell means a
/// card tagged in another is not merely mis-ordered but never fired at all: the
/// fork takes the inline path and steps 1–6 remove the weakness before anything
/// looks at it again. That is the failure mode this fork is least able to
/// report, since a dropped ability leaves no reject behind. Cover Up 01007's
/// game-end trauma is a `when`-cell ability since #720, and the hardcoded
/// `After` here would have silently swallowed it.
/// [`EmitStep::cells`](crate::state::EmitStep::cells) derives the list from the
/// coordinator's own cursor, so a fourth cell cannot be forgotten here.
fn has_weakness_game_end_ability(
    state: &crate::state::GameState,
    investigator: InvestigatorId,
) -> bool {
    crate::state::EmitStep::cells().any(|cell| {
        !super::forced_triggers::collect_forced_hits(
            state,
            &super::forced_triggers::ForcedTriggerPoint::EliminationGameEnd { investigator },
            cell,
        )
        .is_empty()
    })
}

/// Drive a [`Continuation::Elimination`] frame (#638): emit Elimination step 0's
/// weakness-scoped game-end timing point, then — once its abilities have drained
/// above this frame — run steps 1–6 and pop.
///
/// The cursor advances *before* the emit: the emit only queues (ADR 0003), so
/// its frames must land above a frame that is already pointing at its own tail.
pub(super) fn drive_elimination(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::Elimination { investigator, step }) = cx.state.continuations.last_mut()
    else {
        unreachable!("drive_elimination: top frame is not an Elimination");
    };
    let investigator = *investigator;
    match *step {
        EliminationStep::FireWeaknessGameEnd => {
            *step = EliminationStep::RunSteps;
            super::emit::queue_event(
                cx,
                &super::emit::TimingEvent::EliminationGameEnd { investigator },
            )
        }
        EliminationStep::RunSteps => {
            cx.state.continuations.pop();
            // Step 0's tail — "Then, remove those weaknesses from the game" —
            // is step 1's threat-area partition, which removes every owned
            // weakness whether or not it fired.
            run_elimination_steps(cx, investigator);
            check_all_defeated(cx);
            EngineOutcome::Done
        }
    }
}

/// Execute Rules Reference p.10 Elimination steps 1–5 for an
/// investigator whose `status` has just been flipped to a defeated
/// variant. Synchronous: the step-3 re-engagement tie auto-picks the
/// lead rather than suspending (see `reengage_at_location`).
fn run_elimination_steps(cx: &mut Cx, investigator: InvestigatorId) {
    // The location the investigator was at "when eliminated" — read once
    // before any mutations; step 2 deposits clues here.
    let last_location = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location);

    // Step 1, part one: a card in **limbo** is in none of the zones the drains
    // below cover. Two kinds ride a continuation frame rather than a zone, and
    // this walk takes both off their frames:
    //
    // - a card **mid-play** — it left hand when it commenced being played and
    //   has not been placed yet (RR Appendix I step 3 → 4) — #604;
    // - a card **committed to an in-flight skill test** — RR glossary "Limbo":
    //   "A skill card enters limbo as it is committed to a skill test. … It is
    //   no longer considered to be in any investigator's hand, but it has not
    //   yet been placed in any discard pile." (#631.)
    //
    // Taking rather than copying is what makes the step order-independent: the
    // frame's own disposal, whenever it runs, finds nothing left to place
    // instead of pushing the card into a discard pile that step 1 has already
    // removed from the game.
    let mut in_limbo: Vec<crate::state::CardCode> = Vec::new();
    for frame in &mut cx.state.continuations {
        if let Some(card) = frame.take_play_in_progress(investigator) {
            in_limbo.push(card);
        }
        in_limbo.extend(frame.take_committed_cards(investigator));
    }

    // Step 1, part two: remove every card this investigator controls in play and
    // owns in out-of-play areas (hand/deck/discard) from the game.
    //
    // Threat-area cards split by ownership (Rules Reference p.7 names the axis:
    // a defeated card is "placed in the encounter discard pile (or in its
    // owner's discard pile if it is a weakness)"). A **weakness** is owned by
    // this player: step 4 would place it in "the appropriate discard pile", but
    // step 1 removes that very pile from the game one step earlier — so the
    // pile no longer exists and the card is removed. Everything else in the
    // threat area is scenario-owned and is step 4's business (#567).
    let weakness_in_threat_area = |card: &CardInPlay| {
        crate::card_registry::current()
            .and_then(|reg| (reg.metadata_for)(&card.code))
            .is_some_and(|m| m.weakness)
    };
    let inv = cx
        .state
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
    removed.extend(in_limbo);
    removed.extend(inv.cards_in_play.drain(..).map(|c| c.code));
    // Partition the threat area: owned weaknesses leave with their owner here;
    // the rest stay for step 4. No registry installed (engine-only tests with
    // synthetic threat-area cards) ⇒ not a weakness ⇒ step 4.
    let (owned, scenario_owned): (Vec<CardInPlay>, Vec<CardInPlay>) =
        std::mem::take(&mut inv.threat_area)
            .into_iter()
            .partition(weakness_in_threat_area);
    inv.threat_area = scenario_owned;
    removed.extend(owned.into_iter().map(|c| c.code));
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
            if let Some(loc) = cx.state.locations.get_mut(&loc_id) {
                loc.clues = loc.clues.saturating_add(clues);
                let new_count = loc.clues;
                cx.events.push(Event::LocationCluesChanged {
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
    let affected: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    for &eid in &affected {
        let enemy = cx.state.enemies.get_mut(&eid).unwrap_or_else(|| {
            unreachable!("run_elimination_steps: enemy {eid:?} vanished; state corruption")
        });
        enemy.engaged_with = None;
        cx.events.push(Event::EnemyDisengaged {
            enemy: eid,
            investigator,
        });
    }
    for &eid in &affected {
        super::hunters::reengage_at_location(cx, eid);
    }

    // Step 4: "All other cards in the eliminated investigator's threat area are
    // placed in the appropriate discard pile" (Rules Reference p.10). What
    // survives step 1's partition is scenario-owned (Frozen in Fear 01164,
    // Dissonant Voices 01165), so the appropriate pile is the encounter discard
    // — an investigator's elimination must not remove the *scenario's* cards
    // from the game. Engaged enemies are step 3's business, not this drain:
    // they live in `enemies` keyed by `engaged_with`, not in `threat_area`.
    let remaining: Vec<CardInstanceId> = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| inv.threat_area.iter().map(|c| c.instance_id).collect())
        .unwrap_or_default();
    for instance_id in remaining {
        let removed = super::threat_area::discard_from_threat_area(cx, investigator, instance_id);
        debug_assert!(
            removed,
            "elimination step 4: threat-area instance {instance_id:?} vanished mid-drain",
        );
    }

    // Step 5: lead-investigator transfer. No-op by construction: there
    // is no stored lead; `first_active_investigator` recomputes the lead
    // as the first Active investigator in `turn_order`, so a defeated
    // lead is automatically replaced. UX for "remaining players choose"
    // is deferred (Phase 8, #151) alongside the re-engagement-tie pick.

    // Step 6 (no remaining players => scenario ends) is signaled by
    // `check_all_defeated` (caller) emitting AllInvestigatorsDefeated
    // and latching ScenarioEnding::NoResolution; the `apply` hook turns that latch
    // into ScenarioResolved + apply_resolution.

    // The investigator has left play — clear their location last, after
    // step 2 deposited clues using `last_location` (step 3 reads
    // `enemy.current_location` directly, relying on the same value via
    // the engagement invariant).
    let inv = cx
        .state
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
/// Single-source horror application (the Draw-from-empty-deck penalty,
/// treachery/card `Effect::Deal` horror) funnels through this wrapper,
/// which routes through the shared soak entry
/// [`soak_and_place`](super::combat::soak_and_place) (#44/K5a) — so a
/// controlled sanity-bearing asset absorbs the horror, and
/// [`place_assignment`](super::combat::place_assignment) handles the
/// simultaneous-placement + investigator-defeat semantics. The
/// single-source-damage twin [`take_damage`] is symmetric for
/// [`DefeatCause::Damage`]. Enemy attacks (which deal both damage and horror
/// from one source) reach the same entry via
/// [`enemy_attack`](super::combat::enemy_attack).
///
/// [`Status::Insane`]: crate::state::Status::Insane
pub(crate) fn take_horror(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    // Route through the shared soak entry (#44/K5a) so a controlled sanity-bearing
    // asset (Beat Cop, Holy Rosary) absorbs non-attack horror; `place_assignment`
    // applies investigator defeat (cause Horror) when the investigator's share is
    // lethal, preserving this wrapper's prior behaviour.
    //
    // TODO(#728): this places synchronously, so it announces neither
    // `DamageAssigned` nor `DamagePlaced` — an ability keyed to either does not
    // see harm dealt this way. Migrating means parking this caller's tail on a
    // frame first (see `combat::soak_and_place`).
    super::combat::soak_and_place(cx, investigator, 0, amount);
}

/// Apply `amount` damage to `investigator` via the numeric helper,
/// then apply defeat (cause [`DefeatCause::Damage`]) if it was lethal.
/// The single-source-damage twin of `take_horror` — called by
/// `Effect::Deal`'s evaluator (the `HarmKind::Damage` arm).
///
/// Re-exported at `game_core::take_damage` so card-local native effects
/// (#276) can deal damage without re-implementing the defeat check — the
/// first such consumer is Crypt Chill's (01167) no-asset failure branch.
pub fn take_damage(cx: &mut Cx, investigator: InvestigatorId, amount: u8) {
    // Route through the shared soak entry (#44/K5a) so a controlled health-bearing
    // asset (Guard Dog, Beat Cop) absorbs non-attack damage; `place_assignment`
    // applies investigator defeat (cause Damage) when the investigator's share is
    // lethal, preserving this wrapper's prior behaviour.
    //
    // TODO(#728): announces neither condition — see the note on `take_horror`.
    // Dynamite Blast 01024's `for inv in investigators` loop is the caller that
    // makes this the harder of the two to migrate.
    super::combat::soak_and_place(cx, investigator, amount, 0);
}

/// Defeat `investigator` outright by a card ability, with no damage or horror
/// threshold involved — `glossary/Defeat.md`: *"An investigator might also be
/// defeated by a card ability."*
///
/// The card-local (#276) entry point onto the ordinary defeat path: it flips
/// status to [`Status::DefeatedByCardAbility`], announces
/// [`Event::InvestigatorDefeated`] with [`DefeatCause::CardAbility`], and runs
/// Rules Reference p.10 Elimination — including step 6, *"If there are no
/// remaining players, the scenario ends"*, which is how a card that defeats the
/// last active investigator reaches
/// [`ScenarioEnding::NoResolution`](crate::scenario::ScenarioEnding::NoResolution)
/// without latching it itself. Re-exported at `game_core::defeat_investigator`.
///
/// **No-ops on an investigator who is not `Active`** — one who has already been
/// killed, driven insane, or resigned is not defeated again. That is what lets a
/// card printing *"each investigator that has not resigned"* skip the filter:
/// `apply_investigator_defeat`'s own status gate is the filter.
///
/// The twin of [`take_damage`] / `take_horror` for the non-numeric case: those
/// route through the soak entry because damage and horror can be absorbed, and a
/// card-ability defeat cannot be.
pub fn defeat_investigator(cx: &mut Cx, investigator: InvestigatorId) {
    apply_investigator_defeat(cx, investigator, DefeatCause::CardAbility);
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
/// practice; the scenario-ending latch is likewise transition-bounded
/// (first-writer-wins).
///
/// Mutates `state` via the scenario-ending latch (below): on the no-active-
/// investigator transition it requests
/// [`ScenarioEnding::NoResolution`](crate::scenario::ScenarioEnding::NoResolution)
/// per Rules Reference p.10 step 6 — the scenario ended without reaching a
/// resolution point, which is not the same thing as losing it. The `apply`
/// hook turns that latch into [`Event::ScenarioResolved`] +
/// `apply_resolution`.
pub(super) fn check_all_defeated(cx: &mut Cx) {
    let any_active = cx
        .state
        .investigators
        .values()
        .any(|inv| inv.status == Status::Active);
    // Empty-investigators is nonsense scenario state; suppress the
    // event so we don't emit a meaningless "all defeated" when there
    // was nobody to defeat in the first place.
    if !any_active && !cx.state.investigators.is_empty() {
        cx.events.push(Event::AllInvestigatorsDefeated);
        // Rules Reference p.10 step 6: "If there are no remaining players,
        // the scenario ends. Refer to 'no resolution was reached' entry
        // for that scenario in the campaign guide." That is the third
        // ending, not a loss: in campaign play the players "proceed to the
        // next scenario ... regardless of the outcome", and an investigator
        // who got here by resigning is "not considered to have been
        // defeated" (glossary/Resign). First-writer-wins, so an
        // already-fired act/agenda resolution point stays authoritative.
        super::act_agenda::end_scenario(cx.state, crate::scenario::ScenarioEnding::NoResolution);
    }
}

#[cfg(test)]
mod elimination_tests {
    use super::*;
    use crate::assert_event;
    use crate::assert_no_event;
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

    #[test]
    fn elimination_step1_removes_controlled_and_owned_cards() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode("h1".into()), CardCode("h2".into())];
        inv.deck = vec![CardCode("d1".into())];
        inv.discard = vec![CardCode("x1".into())];
        inv.cards_in_play = vec![CardInPlay::enter_play(
            CardCode("p1".into()),
            CardInstanceId(1),
        )];

        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            DefeatCause::Damage,
        );

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
        inv.current_location = Some(loc_id);
        inv.clues = 2;
        inv.resources = 4;

        let mut loc = test_location(1, "Study");
        loc.clues = 1;

        let mut state = GameStateBuilder::default()
            .with_investigator(inv)
            .with_location(loc)
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            DefeatCause::Damage,
        );

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
        dying.current_location = Some(loc);

        let mut survivor = test_investigator(2);
        survivor.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead); // engaged with the about-to-die investigator
            e
        };

        let mut state = GameStateBuilder::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

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
        dying.current_location = Some(loc);

        let enemy = {
            let mut e = test_enemy(1, "Ghoul");
            e.current_location = Some(loc);
            e.engaged_with = Some(dead);
            e
        };

        let mut state = GameStateBuilder::default()
            .with_investigator(dying)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

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
        // scenario-ending latch is set (Rules Reference p.10 step 6).
        crate::test_support::install_test_registry();
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        // After #448 cp2a: max_sanity() reads from the registry (TEST_INV = 8).
        // Pre-load 7 horror so 1 more = 8 = max_sanity → lethal horror.
        investigator.investigator_card.accumulated_horror = 7;
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv])
            .build();
        let mut events = Vec::new();

        // Apply the final point of lethal horror through the standard defeat path.
        take_horror(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            inv,
            1,
        );

        assert_event!(events, Event::AllInvestigatorsDefeated);
        // RR Elimination step 6 is the *third* ending, not a loss: the
        // scenario ended without a resolution point being reached, and the
        // campaign guide answers it under "If no resolution was reached".
        assert_eq!(
            state.ending,
            Some(crate::scenario::ScenarioEnding::NoResolution),
            "no-remaining-players must latch NoResolution, not a resolution point"
        );
    }

    #[test]
    fn elimination_runs_on_horror_defeat_too() {
        let dead = InvestigatorId(1);
        let surv = InvestigatorId(2);
        let loc = LocationId(1);

        let mut dying = test_investigator(1);
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

        let mut state = GameStateBuilder::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Horror,
        );

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

        let mut state = GameStateBuilder::default()
            .with_investigator(dying)
            .with_investigator(survivor)
            .with_location(test_location(1, "Study"))
            .with_enemy(enemy)
            .with_turn_order([dead, surv])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

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
        inv.current_location = None;
        inv.clues = 3;
        inv.resources = 2;

        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            DefeatCause::Damage,
        );

        assert_eq!(
            state.investigators[&id].clues, 0,
            "clues cleared (left play)"
        );
        assert_eq!(state.investigators[&id].resources, 0, "resources returned");
        assert_no_event!(events, Event::LocationCluesChanged { .. });
    }

    #[test]
    fn elimination_without_registry_treats_threat_area_as_scenario_owned() {
        // No registry ⇒ metadata_for is None ⇒ not a weakness ⇒ step 4. The
        // weakness→removed_from_game routing needs real metadata and is covered
        // by `crates/cards/tests/elimination_teardown.rs` (install_test_registry
        // resolves TEST_INV only).
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.threat_area = vec![CardInPlay::enter_play(
            CardCode::new("01165"),
            CardInstanceId(1),
        )];

        let mut state = GameStateBuilder::default().with_investigator(inv).build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            id,
            DefeatCause::Damage,
        );

        assert!(
            state.investigators[&id].threat_area.is_empty(),
            "threat area drained"
        );
        assert_eq!(
            state.encounter_discard.len(),
            1,
            "no registry ⇒ routed to the encounter discard"
        );
        assert!(state.investigators[&id].removed_from_game.is_empty());
    }

    // -------------------------------------------------------------------
    // #764: a defeated active investigator's turn ends (RR Appendix II
    // 2.2.1 → 2.2.2). These cover the arming; `crates/cards/tests/
    // elimination_ends_turn.rs` drives the rotation the flag triggers
    // through the real `apply` loop.
    // -------------------------------------------------------------------

    /// Two investigators mid-`Investigation`, with `whose` holding the open
    /// turn. `dying` is the one about to be defeated.
    fn two_investigator_open_turn(whose: InvestigatorId) -> crate::state::GameState {
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut first = test_investigator(1);
        first.actions_remaining = 2;
        let mut second = test_investigator(2);
        second.actions_remaining = 2;
        GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(first)
            .with_investigator(second)
            .with_active_investigator(whose)
            .with_turn_order([a, b])
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: crate::state::InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(whose)
            .build()
    }

    fn turn_frame(state: &crate::state::GameState) -> Option<(InvestigatorId, bool)> {
        state.continuations.iter().rev().find_map(|c| match c {
            Continuation::InvestigatorTurn {
                investigator,
                ending,
            } => Some((*investigator, *ending)),
            _ => None,
        })
    }

    #[test]
    fn defeat_during_your_own_turn_arms_the_turn_frame_and_announces_the_end() {
        let dead = InvestigatorId(1);
        let mut state = two_investigator_open_turn(dead);
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

        assert_eq!(
            turn_frame(&state),
            Some((dead, true)),
            "RR 2.2.1: an eliminated investigator cannot take an action, so the turn goes to 2.2.2"
        );
        assert_event!(events, Event::TurnEnded { investigator } if *investigator == dead);
        assert_eq!(
            state.investigators[&dead].actions_remaining, 2,
            "the action that killed them stays charged — unlike `end_turn`, this does not drain"
        );
    }

    #[test]
    fn defeat_outside_your_turn_leaves_the_active_investigators_frame_alone() {
        // Dynamite Blast 01024 catching a co-located investigator, or an Enemy
        // phase attack: the turn frame belongs to someone else and must not be
        // armed — arming it would end the *survivor's* turn.
        let (active, dead) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = two_investigator_open_turn(active);
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

        assert_eq!(
            turn_frame(&state),
            Some((active, false)),
            "the active investigator's turn is untouched by someone else's defeat"
        );
        assert_no_event!(events, Event::TurnEnded { .. });
    }

    #[test]
    fn defeat_with_no_turn_frame_at_all_is_a_no_op() {
        // The Mythos phase (Rotting Remains 01163) and the Enemy phase: no
        // `InvestigatorTurn` frame exists, so there is nothing to end.
        let dead = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_investigator(test_investigator(1))
            .with_turn_order([dead])
            .build();
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Horror,
        );

        assert_eq!(turn_frame(&state), None);
        assert_no_event!(events, Event::TurnEnded { .. });
    }

    #[test]
    fn defeat_while_the_turn_is_already_ending_does_not_re_announce_it() {
        // The player submitted `EndTurn` and a suspending `EndOfTurn` forced
        // ability (Frozen in Fear 01164's willpower test) killed them. `end_turn`
        // already emitted `TurnEnded`; the frame stays armed exactly once and the
        // end is announced exactly once.
        let dead = InvestigatorId(1);
        let mut state = two_investigator_open_turn(dead);
        for c in &mut state.continuations {
            if let Continuation::InvestigatorTurn { ending, .. } = c {
                *ending = true;
            }
        }
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Damage,
        );

        assert_eq!(turn_frame(&state), Some((dead, true)));
        assert_no_event!(events, Event::TurnEnded { .. });
    }

    #[test]
    fn resigning_during_your_own_turn_ends_it_too() {
        // RR "Resign": an investigator who resigns "is eliminated by
        // resignation" and "is not considered to have been defeated" — but
        // eliminated all the same, so 2.2.1 sends the turn to 2.2.2 either way.
        //
        // Forward-looking: no Resign action exists yet (`DefeatCause::Resigned`
        // is still "a placeholder slot until the Resign action lands"), so this
        // pins the behaviour for when one does rather than covering live code.
        let dead = InvestigatorId(1);
        let mut state = two_investigator_open_turn(dead);
        let mut events = Vec::new();

        apply_investigator_defeat(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            dead,
            DefeatCause::Resigned,
        );

        assert_eq!(state.investigators[&dead].status, Status::Resigned);
        assert_eq!(turn_frame(&state), Some((dead, true)));
    }
    /// `glossary/Defeat.md`: *"An investigator might also be defeated by a card
    /// ability."* That defeat is neither killed nor driven insane — the same
    /// entry makes those two consequences of **trauma** — so it carries its own
    /// cause and its own status.
    #[test]
    fn a_card_ability_defeat_is_neither_killed_nor_insane() {
        crate::test_support::install_test_registry();
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = two_investigator_open_turn(a);
        let mut events = Vec::new();

        defeat_investigator(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            a,
        );

        assert_eq!(
            state.investigators[&a].status,
            Status::DefeatedByCardAbility,
            "not Killed (damage) and not Insane (horror)",
        );
        assert_event!(events, Event::InvestigatorDefeated { investigator, cause }
            if *investigator == a && *cause == DefeatCause::CardAbility);
        assert_eq!(
            state.investigators[&b].status,
            Status::Active,
            "the other investigator is untouched",
        );
        assert!(
            state.ending.is_none(),
            "one active investigator remains, so the scenario has not ended",
        );
    }

    /// Elimination step 6, *"If there are no remaining players, the scenario
    /// ends"* — reached through the ordinary defeat path, so a card ability that
    /// drains the last active investigator ends the scenario at **no** resolution
    /// point without latching one itself.
    #[test]
    fn a_card_ability_defeating_the_last_investigator_latches_no_resolution() {
        crate::test_support::install_test_registry();
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = two_investigator_open_turn(a);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        defeat_investigator(&mut cx, a);
        defeat_investigator(&mut cx, b);

        assert_event!(events, Event::AllInvestigatorsDefeated);
        assert_eq!(
            state.ending,
            Some(crate::scenario::ScenarioEnding::NoResolution),
            "Elimination step 6, not a numbered resolution",
        );
    }

    /// An investigator who is not `Active` is not defeated again — which is what
    /// lets a card printing *"each investigator that has not resigned"* skip the
    /// filter entirely.
    #[test]
    fn a_card_ability_defeat_no_ops_on_an_already_eliminated_investigator() {
        crate::test_support::install_test_registry();
        let (a, b) = (InvestigatorId(1), InvestigatorId(2));
        let mut state = two_investigator_open_turn(b);
        state.investigators.get_mut(&a).expect("seated").status = Status::Resigned;
        let mut events = Vec::new();

        defeat_investigator(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            a,
        );

        assert_eq!(
            state.investigators[&a].status,
            Status::Resigned,
            "the resigned investigator is left alone",
        );
        assert_no_event!(events, Event::InvestigatorDefeated { .. });
    }
}
