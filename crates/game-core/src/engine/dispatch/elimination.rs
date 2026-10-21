//! Investigator elimination helpers: defeat application, elimination
//! steps, horror application, and no-remaining-players detection.

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, NotLeft, Placement};
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{act_agenda, cursor, emit, engagement, trigger_scan};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::{Event, TraumaKind};
use crate::scenario::ScenarioEnding;
use crate::state::{
    CardCode, CardInstanceId, EliminationCause, EliminationFrame, EliminationStep, EmitStep,
    EnemyId, GameState, InvestigatorId, Owner, Status,
};
#[cfg(test)]
use crate::state::{LocationId, Phase};

/// Flip an Active investigator's status to the variant `cause` implies —
/// [`Status::Resigned`] for a resignation, [`Status::Defeated`] for every
/// defeat — and emit [`Event::InvestigatorEliminated`]. No-op if the
/// investigator is already non-Active: an investigator is eliminated once, and
/// only once.
///
/// **The three defeat causes all land on [`Status::Defeated`]**. Killed
/// and driven insane are campaign-log states derived from accumulated trauma
/// totals, not from how one scenario defeat happened —
/// `glossary/Campaign_Play.md`: *"If an investigator has physical trauma equal
/// to his or her printed health, the investigator is killed."* The cause is not
/// lost: it rides `cause` on the event, which is the campaign log's input.
///
/// **Elimination, not defeat.** `glossary/Elimination.md` opens *"A player is
/// eliminated from a scenario any time his or her investigator is defeated, **or
/// if he or she resigns**"* and then gives the steps once, so this one helper
/// serves both — the four [`EliminationCause`]s differ in the status they land
/// on and in nothing else. [`EliminationCause::Resigned`] in particular is *not*
/// a defeat (`glossary/Resign.md`), which is why the umbrella term names the
/// function and the event; see [`resign_investigator`].
///
/// # Then one of two paths
///
/// - **No step-0 weakness ability** (every elimination but a Roland holding
///   clues on Cover Up): [`run_elimination_steps`] and [`check_all_eliminated`]
///   run inline before this returns.
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

    // If it was their turn, that turn is over.
    end_turn_on_elimination(cx, investigator);

    // Rules Reference p.10 Elimination step 0. The rule, why the steps
    // have to ride a frame to honour it, and what that costs are all documented
    // once on `Continuation::Elimination`; this is the fork it describes.
    if has_weakness_game_end_ability(cx.state, cx.cards, investigator) {
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

/// End the active investigator's turn when elimination takes them out of
/// it: announce [`Event::TurnEnded`] and arm their
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
/// game-end trauma is a `when`-cell ability, which a hardcoded `After` here
/// would silently swallow.
/// [`EmitStep::cells`](crate::state::EmitStep::cells) derives the list from the
/// coordinator's own cursor, so a fourth cell cannot be forgotten here.
fn has_weakness_game_end_ability(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
) -> bool {
    EmitStep::cells().any(|cell| {
        !trigger_scan::collect_forced(
            state,
            cards,
            &TimingEvent::EliminationGameEnd { investigator },
            cell,
        )
        .is_empty()
    })
}

/// Drive a [`Continuation::Elimination`] frame: emit Elimination step 0's
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
        theirs.extend(frame.take_resolving_weakness(investigator));
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
    drain(cx, leaving, "step 1", |cx, id| {
        board::remove_from_game_on_elimination(cx, id)
    });
}

/// Take each of `leaving` out of play through `exit`, in order. The ids are
/// collected before the first card leaves, so the drain never reads a zone it
/// is emptying; `step` names the elimination step in the assertion that each
/// card was still there when its turn came.
///
/// Both drains take an elimination exit, which overrides a card's *"cannot
/// leave play"*: step 0, *"the game has ended for the eliminated investigator
/// … Then, remove those weaknesses from the game"*, so The Necronomicon 01009
/// leaves with its horror still on it.
fn drain(
    cx: &mut Cx,
    leaving: Vec<CardInstanceId>,
    step: &str,
    exit: fn(&mut Cx, CardInstanceId) -> Result<Placement, NotLeft>,
) {
    for instance_id in leaving {
        let left = exit(cx, instance_id);
        debug_assert!(
            left.is_ok(),
            "elimination {step}: instance {instance_id:?} vanished mid-drain",
        );
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
    // below cover. Three kinds ride a continuation frame rather than a zone, and
    // this walk takes each off its frame:
    //
    // - a card **mid-play** — it left hand when it commenced being played and
    //   has not been placed yet (RR Appendix I step 3 → 4);
    // - a card **committed to an in-flight skill test** — RR glossary "Limbo":
    //   "A skill card enters limbo as it is committed to a skill test. … It is
    //   no longer considered to be in any investigator's hand, but it has not
    //   yet been placed in any discard pile."
    // - an **encounter-cardtype weakness of theirs resolving** as an encounter
    //   card — RR glossary "Limbo": "A treachery card enters limbo after it is
    //   drawn, while its revelation ability is being resolved." Only their own:
    //   a card from the encounter deck is the scenario's, and its frame's
    //   disposal still files it.
    //
    // Taking rather than copying is what makes the step order-independent: the
    // frame's own disposal, whenever it runs, finds nothing left to place
    // instead of pushing the card into a discard pile that step 1 has already
    // removed from the game.
    //
    // A card in limbo comes back with its **owner**, and is filed by it for the
    // same reason a card leaving play is: the eliminated investigator's
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
    // (`GameState::removed_from_game`). See **Removed from game** in
    // `GLOSSARY.md` for why the two piles differ.
    //
    // The threat area splits by the same **owner**: a card this investigator
    // owns there (a weakness such as Cover Up 01007, the bearer's) leaves with
    // them now. Step 4 would place it in "the appropriate discard pile", but
    // step 1 removes that very pile from the game one step earlier, so the
    // card is removed. Every other threat-area card is step 4's business.
    //
    // Each card leaving play emits its one removal event. The out-of-play
    // areas and limbo emit none: those cards were never in play.
    //
    // TODO(#1132): an enemy this investigator owns, such as the weakness Mob
    // Enforcer 01101, lives in `state.enemies`, which this step doesn't
    // touch, so it stays in play with a bearer who is gone.
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
    // at when he or she was eliminated, unengaged but otherwise
    // maintaining their current game state" (RR p.10). Engaged enemies
    // already share the investigator's location by the engagement invariant
    // (Move drags them along), so no location update is needed — just clear
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
        engagement::disengage(cx, eid);
    }
    for &eid in &affected {
        engagement::reengage_at_location(cx, eid);
    }

    // Step 4: "All other cards in the eliminated investigator's threat area are
    // placed in the appropriate discard pile" (Rules Reference p.10) — each
    // card's owner's, through the leave-play exit: an encounter treachery
    // (Frozen in Fear 01164, Dissonant Voices 01165) to the encounter discard,
    // so an investigator's elimination does not remove the *encounter deck's*
    // cards from the game. Engaged enemies are step 3's business, not this drain:
    // they live in `enemies` keyed by `engaged_with`, not in `threat_area`.
    let remaining: Vec<CardInstanceId> = cx
        .state
        .investigators
        .get(&investigator)
        .map(|inv| inv.threat_area.iter().map(|c| c.instance_id).collect())
        .unwrap_or_default();
    drain(cx, remaining, "step 4", |cx, id| {
        board::discard_from_play_on_elimination(cx, id)
    });

    // Step 5: lead-investigator transfer. No-op by construction: the engine
    // stores no lead, and its proxy (`first_active_investigator`, the first
    // Active investigator in `turn_order`) passes to the next seat when this
    // one is defeated. The remaining players' choice of a new lead is not
    // modelled; see the proxy under **Lead investigator** in `GLOSSARY.md`.

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

/// Defeat `investigator` outright by a card ability, with no damage or horror
/// threshold involved — `glossary/Defeat.md`: *"An investigator might also be
/// defeated by a card ability."*
///
/// The card-local entry point onto the ordinary defeat path: it flips
/// status to [`Status::Defeated`], announces
/// [`Event::InvestigatorEliminated`] with [`EliminationCause::CardAbility`], and runs
/// Rules Reference p.10 Elimination — including step 6, *"If there are no
/// remaining players, the scenario ends"*, which is how a card that defeats the
/// last active investigator reaches
/// [`ScenarioEnding::NoResolution`](crate::scenario::ScenarioEnding::NoResolution)
/// without latching it itself. A card-local native reaches it as
/// [`NativeCx::defeat_investigator`](crate::engine::NativeCx::defeat_investigator).
///
/// **No-ops on an investigator who is not `Active`** — one who has already been
/// killed, driven insane, or resigned is not defeated again. That is what lets a
/// card printing *"each investigator that has not resigned"* skip the filter:
/// `apply_investigator_elimination`'s own status gate is the filter.
///
/// The non-numeric counterpart of [`deal_harm`](super::combat::deal_harm): harm
/// is distributed because damage and horror can be absorbed, and a card-ability
/// defeat cannot be.
pub fn defeat_investigator(cx: &mut Cx, investigator: InvestigatorId) {
    apply_investigator_elimination(cx, investigator, EliminationCause::CardAbility);
}

/// `investigator` suffers `amount` trauma of `kind`: emits
/// [`Event::TraumaSuffered`]. Card-local natives (They're Getting Out 01107,
/// Cover Up 01007) reach it as
/// [`NativeCx::suffer_trauma`](crate::engine::NativeCx::suffer_trauma).
///
/// **Announced, not recorded.** Nothing persists trauma yet, and this is the
/// one place it will be recorded. TODO(#766): record it in the campaign log.
pub fn suffer_trauma(cx: &mut Cx, investigator: InvestigatorId, kind: TraumaKind, amount: u8) {
    cx.events.push(Event::TraumaSuffered {
        investigator,
        kind,
        amount,
    });
}

/// Resign `investigator` from the scenario — what the
/// [`Resign`](card_dsl::dsl::ActionDesignator::Resign) action designator
/// performs, and the only producer of [`EliminationCause::Resigned`].
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
