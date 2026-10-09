//! Investigator elimination helpers: defeat application, elimination
//! steps, horror application, and no-remaining-players detection.

use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{act_agenda, combat, cursor, emit, hunters, trigger_scan};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{board, Cx};
use crate::event::Event;
use crate::scenario::ScenarioEnding;
use crate::state::{
    CardCode, CardInstanceId, EliminationCause, EliminationFrame, EliminationStep, EmitStep,
    EnemyId, GameState, InvestigatorId, Owner, Status,
};
#[cfg(test)]
use crate::state::{CardInPlay, LocationId, Phase};

/// Flip an Active investigator's status to the variant `cause` implies —
/// [`Status::Resigned`] for a resignation, [`Status::Defeated`] for every
/// defeat — and emit [`Event::InvestigatorEliminated`]. No-op if the
/// investigator is already non-Active: an investigator is eliminated once, and
/// only once.
///
/// **The three defeat causes all land on [`Status::Defeated`]** (#814). Killed
/// and driven insane are campaign-log states derived from accumulated trauma
/// totals, not from how one scenario defeat happened —
/// `glossary/Campaign_Play.md`: *"If an investigator has physical trauma equal
/// to his or her printed health, the investigator is killed."* The cause is not
/// lost: it rides `cause` on the event, which is the campaign log's input
/// (#766).
///
/// **Elimination, not defeat.** `glossary/Elimination.md` opens *"A player is
/// eliminated from a scenario any time his or her investigator is defeated, **or
/// if he or she resigns**"* and then gives the steps once, so this one helper
/// serves both — the four [`EliminationCause`]s differ in the status they land
/// on and in nothing else. [`EliminationCause::Resigned`] in particular is *not*
/// a defeat (`glossary/Resign.md`), which is why the umbrella term names the
/// function and the event; see [`resign_investigator`].
///
/// # Then one of two paths (#638)
///
/// - **No step-0 weakness ability** (every elimination but a Roland holding
///   clues on Cover Up): [`run_elimination_steps`] and [`check_all_eliminated`]
///   run inline before this returns, as they always have.
/// - **A step-0 weakness ability**: a [`Continuation::Elimination`] frame is
///   pushed and this returns immediately. Steps 1–6 *and*
///   [`check_all_eliminated`] run later, from [`drive_elimination`], once the
///   queued abilities have drained — so on this path a caller that resumes
///   after this function sees an elimination still **in progress**: status
///   flipped, but cards not yet removed and no `AllInvestigatorsEliminated` /
///   `ScenarioEnding` latch yet. [`super::combat::place_assignment`] is the
///   only such caller today and gates on [`Status`] for exactly this reason.
///
/// [`Status`]: crate::state::Status
pub(super) fn apply_investigator_elimination(
    cx: &mut Cx,
    investigator: InvestigatorId,
    cause: EliminationCause,
) {
    let inv = cx.state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "apply_investigator_elimination: investigator {investigator:?} is not in the investigators map; \
             this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return;
    }
    inv.status = match cause {
        EliminationCause::Resigned => Status::Resigned,
        EliminationCause::Damage | EliminationCause::Horror | EliminationCause::CardAbility => {
            Status::Defeated
        }
    };
    cx.events.push(Event::InvestigatorEliminated {
        investigator,
        cause,
    });

    // If it was their turn, that turn is over (#764).
    end_turn_on_elimination(cx, investigator);

    // Rules Reference p.10 Elimination step 0 (#638). The rule, why the steps
    // have to ride a frame to honour it, and what that costs are all documented
    // once on `Continuation::Elimination`; this is the fork it describes.
    if has_weakness_game_end_ability(cx.state, investigator) {
        cx.state.continuations.push(EliminationFrame {
            investigator,
            step: EliminationStep::FireWeaknessGameEnd,
        });
        return;
    }

    // Rules Reference p.10 Elimination steps 1–5 run here, between the
    // elimination event and the step-6 check. See the design doc
    // 2026-05-31-144 for the full breakdown.
    run_elimination_steps(cx, investigator);

    check_all_eliminated(cx);
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
/// step-0 weakness path, where [`apply_investigator_elimination`] returns before
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
/// `InvestigatorEliminated` → `TurnEnded` → the teardown that follows.
///
/// # All investigators eliminated
///
/// The armed frame is never resumed: `check_all_eliminated` latches
/// `ScenarioEnding::NoResolution`, and an `InvestigatorTurn` is
/// [cancelled by a latched resolution](Continuation::cancelled_by_scenario_end),
/// so `drive` pops it rather than rotating (ADR 0004). A solo elimination —
/// defeat or resignation alike — therefore ends the scenario, and this arming is
/// what keeps a *surviving* table moving.
fn end_turn_on_elimination(cx: &mut Cx, investigator: InvestigatorId) {
    let Some(ending) = cursor::turn_frame_ending_mut(cx.state, investigator) else {
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
/// fork in [`apply_investigator_elimination`] cannot drift from what the scan
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
fn has_weakness_game_end_ability(state: &GameState, investigator: InvestigatorId) -> bool {
    EmitStep::cells().any(|cell| {
        !trigger_scan::collect_forced(
            state,
            &TimingEvent::EliminationGameEnd { investigator },
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
    let frame = cx.state.continuations.top_mut::<EliminationFrame>();
    let investigator = frame.investigator;
    match frame.step {
        EliminationStep::FireWeaknessGameEnd => {
            frame.step = EliminationStep::RunSteps;
            emit::queue_event(cx, &TimingEvent::EliminationGameEnd { investigator })
        }
        EliminationStep::RunSteps => {
            cx.state.continuations.pop_expect::<EliminationFrame>();
            // Step 0's tail — "Then, remove those weaknesses from the game" —
            // is step 1's threat-area partition, which removes every owned
            // weakness whether or not it fired.
            run_elimination_steps(cx, investigator);
            check_all_eliminated(cx);
            EngineOutcome::Done
        }
    }
}

/// Execute Rules Reference p.10 Elimination steps 1–5 for an
/// investigator whose `status` has just been flipped to a defeated
/// variant. Synchronous: the step-3 re-engagement tie auto-picks the
/// lead rather than suspending (see `reengage_at_location`).
/// Take every card of `investigator`'s that is in **no zone** off the frames
/// holding it, split by owner: `(theirs, the scenario's)`.
///
/// Split out of [`run_elimination_steps`] to keep it under the function-size
/// lint; the *why* — a card mid-play or in limbo is reachable by no zone drain —
/// is at the call site.
fn take_limbo_cards(cx: &mut Cx, investigator: InvestigatorId) -> (Vec<CardCode>, Vec<CardCode>) {
    let mut theirs = Vec::new();
    let mut scenarios = Vec::new();
    for frame in cx.state.continuations.frames_mut() {
        if let Some((card, owner)) = frame.take_play_in_progress(investigator) {
            if owner == Owner::Investigator(investigator) {
                theirs.push(card);
            } else {
                scenarios.push(card);
            }
        }
        theirs.extend(frame.take_committed_cards(investigator));
    }
    (theirs, scenarios)
}

/// Step 1's in-play half: remove from the game, through the leave-play exit,
/// every card in `investigator`'s play area and every card they own in their
/// threat area — each filed by its owner, with its one removal event.
///
/// Split out of [`run_elimination_steps`] to keep it under the function-size
/// lint; the *why* is at the call site.
fn remove_cards_in_play(cx: &mut Cx, investigator: InvestigatorId) {
    let leaving: Vec<CardInstanceId> = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| {
            inv.cards_in_play
                .iter()
                .chain(
                    inv.threat_area
                        .iter()
                        .filter(|card| card.owner == Owner::Investigator(investigator)),
                )
                .map(|card| card.instance_id)
                .collect()
        })
        .unwrap_or_default();
    for instance_id in leaving {
        let left = board::remove_from_game(cx, instance_id);
        debug_assert!(
            left.is_some(),
            "elimination step 1: instance {instance_id:?} vanished mid-drain",
        );
    }
}

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
    //
    // A card in limbo comes back with its **owner**, and is filed by it for the
    // same reason a card leaving play is (#772): the eliminated investigator's
    // own pile is for their own deck's cards, and a card mid-take-control on a
    // `SlotDiscard` frame is the scenario's. A committed skill card is always
    // its own player's — it came from their hand.
    let (in_limbo, scenario_owned_limbo) = take_limbo_cards(cx, investigator);

    // Step 1, part two: remove every card this investigator controls in play and
    // owns in out-of-play areas (hand/deck/discard) from the game.
    //
    // *"The cards he or she **controls** in play … are removed from the game"* —
    // so every card in the play area leaves it, whoever owns it, through the
    // leave-play exit, which files each by its **owner**: this investigator's
    // own cards to their removed-from-game pile, and a card they merely control
    // — Lita Chantler 01117 after a Parley — to the scenario's
    // (`GameState::removed_from_game`, #772). See **Removed from game** in
    // `GLOSSARY.md` for why the two piles differ.
    //
    // The threat area splits by the same **owner**: a card this investigator
    // owns there (a weakness such as Cover Up 01007, the bearer's) leaves with
    // them now. Step 4 would place it in "the appropriate discard pile", but
    // step 1 removes that very pile from the game one step earlier, so the
    // card is removed. Every other threat-area card is step 4's business
    // (#567).
    //
    // Each card leaving play emits its one removal event. The out-of-play
    // areas and limbo emit none: those cards were never in play.
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
            )
        });
    inv.removed_from_game.extend(in_limbo);
    remove_cards_in_play(cx, investigator);
    cx.state.removed_from_game.extend(scenario_owned_limbo);
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "run_elimination_steps: investigator {investigator:?} not in map; state corruption"
            )
        });
    inv.removed_from_game.append(&mut inv.hand);
    inv.removed_from_game.append(&mut inv.deck);
    inv.removed_from_game.append(&mut inv.discard);

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
        hunters::reengage_at_location(cx, eid);
    }

    // Step 4: "All other cards in the eliminated investigator's threat area are
    // placed in the appropriate discard pile" (Rules Reference p.10) — each
    // card's owner's, through the leave-play exit: an encounter treachery
    // (Frozen in Fear 01164, Dissonant Voices 01165) to the encounter discard,
    // so an investigator's elimination does not remove the *scenario's* cards
    // from the game. Engaged enemies are step 3's business, not this drain:
    // they live in `enemies` keyed by `engaged_with`, not in `threat_area`.
    let remaining: Vec<CardInstanceId> = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| inv.threat_area.iter().map(|c| c.instance_id).collect())
        .unwrap_or_default();
    for instance_id in remaining {
        let left = board::discard_from_play(cx, instance_id);
        debug_assert!(
            left.is_some(),
            "elimination step 4: threat-area instance {instance_id:?} vanished mid-drain",
        );
    }

    // Step 5: lead-investigator transfer. No-op by construction: there
    // is no stored lead; `first_active_investigator` recomputes the lead
    // as the first Active investigator in `turn_order`, so a defeated
    // lead is automatically replaced. UX for "remaining players choose"
    // is deferred (Phase 8, #151) alongside the re-engagement-tie pick.

    // Step 6 (no remaining players => scenario ends) is signaled by
    // `check_all_eliminated` (caller) emitting AllInvestigatorsEliminated
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
/// horror reaches `max_sanity`, flip status to [`Status::Defeated`],
/// emit [`Event::InvestigatorEliminated`], and (if no `Active`
/// investigators remain) emit [`Event::AllInvestigatorsEliminated`].
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
/// [`EliminationCause::Damage`]. Enemy attacks (which deal both damage and horror
/// from one source) reach the same entry via
/// [`enemy_attack`](super::combat::enemy_attack).
///
/// [`Status::Defeated`]: crate::state::Status::Defeated
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
    combat::soak_and_place(cx, investigator, 0, amount);
}

/// Apply `amount` damage to `investigator` via the numeric helper,
/// then apply defeat (cause [`EliminationCause::Damage`]) if it was lethal.
/// The single-source-damage twin of `take_horror` — called by
/// `Effect::Deal`'s evaluator (the `HarmKind::Damage` arm).
///
/// Re-exported at `game_core::engine::take_damage` so card-local native effects
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
    combat::soak_and_place(cx, investigator, amount, 0);
}

/// Defeat `investigator` outright by a card ability, with no damage or horror
/// threshold involved — `glossary/Defeat.md`: *"An investigator might also be
/// defeated by a card ability."*
///
/// The card-local (#276) entry point onto the ordinary defeat path: it flips
/// status to [`Status::Defeated`], announces
/// [`Event::InvestigatorEliminated`] with [`EliminationCause::CardAbility`], and runs
/// Rules Reference p.10 Elimination — including step 6, *"If there are no
/// remaining players, the scenario ends"*, which is how a card that defeats the
/// last active investigator reaches
/// [`ScenarioEnding::NoResolution`](crate::scenario::ScenarioEnding::NoResolution)
/// without latching it itself. Re-exported at `game_core::engine::defeat_investigator`.
///
/// **No-ops on an investigator who is not `Active`** — one who has already been
/// killed, driven insane, or resigned is not defeated again. That is what lets a
/// card printing *"each investigator that has not resigned"* skip the filter:
/// `apply_investigator_elimination`'s own status gate is the filter.
///
/// The twin of [`take_damage`] / `take_horror` for the non-numeric case: those
/// route through the soak entry because damage and horror can be absorbed, and a
/// card-ability defeat cannot be.
pub fn defeat_investigator(cx: &mut Cx, investigator: InvestigatorId) {
    apply_investigator_elimination(cx, investigator, EliminationCause::CardAbility);
}

/// Resign `investigator` from the scenario — what the
/// [`Resign`](card_dsl::dsl::ActionDesignator::Resign) action designator performs
/// (#805), and the only producer of [`EliminationCause::Resigned`].
///
/// `glossary/Resign.md`: *"When an investigator resigns, the investigator is
/// eliminated by resignation (see 'Elimination' on page 10.) An investigator
/// who resigns is not considered to have been defeated."*
///
/// **One procedure, no branch.** `glossary/Elimination.md` opens *"A player is
/// eliminated from a scenario any time his or her investigator is defeated, **or
/// if he or she resigns**"* and then gives steps 0–6 once, so this runs the
/// identical path [`defeat_investigator`] does and differs only in the cause it
/// carries. Two consequences worth naming, both from that shared path:
///
/// - Step 2 places the resigner's clues **at the location they resigned from**,
///   so walking out of the Parlor 01115 leaves those clues on the board for act
///   1 to keep counting.
/// - Step 6 — *"If there are no remaining players, the scenario ends"* — is
///   reached through [`check_all_eliminated`], so the last investigator resigning
///   ends the scenario at
///   [`ScenarioEnding::NoResolution`](crate::scenario::ScenarioEnding::NoResolution)
///   rather than at a resolution point. That ending is **not** a loss; see the
///   **No resolution reached** entry in `GLOSSARY.md`.
///
/// **No-ops on an investigator who is not `Active`**, via
/// `apply_investigator_elimination`'s own status gate.
pub(crate) fn resign_investigator(cx: &mut Cx, investigator: InvestigatorId) {
    apply_investigator_elimination(cx, investigator, EliminationCause::Resigned);
}

/// Emit [`Event::AllInvestigatorsEliminated`] when no `Active`
/// investigator remains — Rules Reference p.10 Elimination step 6,
/// *"If there are no remaining players, the scenario ends."*
///
/// The question is *remaining*, not *defeated*: an investigator who resigned is
/// gone from the scenario without ever having been defeated
/// (`glossary/Resign.md`), and the last one doing so ends it just as a defeat
/// would.
///
/// **Contract for callers:** *any* code path that flips a
/// `Status::Active` investigator to a non-`Active` status (Defeated,
/// Resigned) must call this helper afterwards. Currently the
/// only status-flipping path is [`apply_investigator_elimination`], so
/// that one helper is the only caller; future paths that flip status
/// outside this helper (a scenario effect that bypasses the standard
/// elimination-cause routing) need to add a call too — otherwise the event
/// silently fails to fire when those paths take the last `Active`
/// investigator out of the scenario.
///
/// Idempotent on subsequent eliminations: the predicate becomes true at the
/// first no-remaining-players transition and stays true. Callers only invoke it
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
pub(super) fn check_all_eliminated(cx: &mut Cx) {
    let any_active = cx
        .state
        .investigators
        .values()
        .any(|inv| inv.status == Status::Active);
    // Empty-investigators is nonsense scenario state; suppress the
    // event so we don't emit a meaningless "all eliminated" when there
    // was nobody in the scenario in the first place.
    if !any_active && !cx.state.investigators.is_empty() {
        cx.events.push(Event::AllInvestigatorsEliminated);
        // Rules Reference p.10 step 6: "If there are no remaining players,
        // the scenario ends. Refer to 'no resolution was reached' entry
        // for that scenario in the campaign guide." That is the third
        // ending, not a loss: in campaign play the players "proceed to the
        // next scenario ... regardless of the outcome", and an investigator
        // who got here by resigning is "not considered to have been
        // defeated" (glossary/Resign). First-writer-wins, so an
        // already-fired act/agenda resolution point stays authoritative.
        act_agenda::end_scenario(cx.state, ScenarioEnding::NoResolution);
    }
}

#[cfg(test)]
mod tests;
