//! The Move basic action, and the departure and enter steps a move
//! resolves through.

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{emit, hunters, movement, reveal};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResume, EnemyId, GameState, InvestigatorId, LocationId, MoveEnterFrame};

/// The destinations a Move may take `investigator` to, in the order their
/// current location lists its connections. `glossary/Move_Action.md`: *"move
/// that investigator … to any other location that is marked as a connecting
/// location on his or her current location"*. A connection to a location not
/// in state (malformed scenario data) is no destination, and neither is one a
/// movement barrier blocks (#774): the barrier filters the step, never the
/// graph, so the connection stays on the map for everything that measures
/// distance across it.
///
/// Read by the basic action's validation, its post-`AoO` re-check, and the turn
/// menu. Empty for a locationless investigator, and for one whose
/// `current_location` dangles: that corruption is the handler's to surface
/// loudly, so the menu offers nothing rather than panicking. A designated
/// **Move** is not implemented yet
/// ([`designated_unimplemented`](super::designated_unimplemented)).
pub(crate) fn candidates(state: &GameState, investigator: InvestigatorId) -> Vec<LocationId> {
    let Some(from) = state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
    else {
        return Vec::new();
    };
    let Some(from_loc) = state.locations.get(&from) else {
        return Vec::new();
    };
    from_loc
        .connections
        .iter()
        .copied()
        .filter(|&dest| {
            dest != from
                && state.locations.contains_key(&dest)
                && movement::investigator_can_enter_location(state, dest)
        })
        .collect()
}

/// Handler for `TurnAction::Move`.
///
/// Spends 1 action, then updates `current_location` to a connected
/// destination. Move is legal while engaged with enemies: per the
/// Rules Reference, each ready engaged enemy makes an attack of
/// opportunity before the move resolves, and engaged enemies move
/// with the investigator. Both behaviors land alongside enemy state
/// in #67; this handler covers only the bare movement.
///
/// Validate-first: the investigator may take the action, surcharge included
/// ([`take::check`]), then the destination checks. Then take it
/// ([`take::take`]). Move is not on the attack-of-opportunity exempt list, so
/// each ready engaged enemy attacks before [`perform`] relocates.
///
/// Move's share of the #818 blocker
/// ([`designated_unimplemented`](super::designated_unimplemented)): the
/// `ActionDesignator::Move` variant carries no modification because no corpus
/// card prints a bold **Move** at all, so there is no printed shape to take one
/// from. Printings outside the corpus suggest a destination or a repeat count
/// rather than a stat row (#818 lists them). When #818 lands, a designated Move
/// performs through this module as the basic Move does.
pub(in crate::engine::dispatch) fn handle(
    cx: &mut Cx,
    investigator: InvestigatorId,
    destination: LocationId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Move);
    if let Err(reason) = take::check(cx.state, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let inv = &cx.state.investigators[&investigator];
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
    // an invariant violation and panics, ahead of the candidates check (which
    // is empty on corrupt state rather than panicking). Connection lists, by
    // contrast, are scenario-data inputs — a connection pointing at a missing
    // location is malformed input, not engine corruption, so we reject.
    let from_loc = cx.state.locations.get(&from).unwrap_or_else(|| {
        unreachable!(
            "Move: location {from:?} (investigator's current_location) is not in the \
             locations map; this is a state-corruption invariant violation"
        )
    });
    if !candidates(cx.state, investigator).contains(&destination) {
        // Say why, destination-in-state first so the message is informative
        // when several fail. The barrier (#774) is the last reason: the `apply`
        // seam is submittable directly, so a client that never read the menu,
        // or read a stale one, must still be refused. The Parlor 01115's
        // unrevealed back is the only card in the corpus that prints one.
        let reason = if !cx.state.locations.contains_key(&destination) {
            format!("Move: destination {destination:?} is not in state")
        } else if !from_loc.connections.contains(&destination) {
            format!("Move: {destination:?} is not connected to {from:?}")
        } else {
            format!("Move: movement into {destination:?} is blocked by a card ability")
        };
        return EngineOutcome::Rejected {
            reason: reason.into(),
        };
    }

    // Mutate-second: take the action last, after every move precondition has
    // passed, so a rejected move spends nothing.
    take::take(cx, investigator, &description, |_| {
        Ok(ActionResume::Move { destination })
    })
}

/// The relocation half of a Move, run after its attack-of-opportunity loop
/// completes (#293). Re-derives `from` from the live `current_location` (the `AoO`
/// never moves the actor) and re-checks the destination is still one of the
/// [`candidates`], so still connected and still enterable — the §D
/// primary-precondition re-check — suppressing the move (returns `Done`) if it
/// no longer is. The barrier re-check is
/// there for the same reason the connection one is: the `AoO` loop can resolve
/// arbitrary card effects between validation and relocation.
///
/// Then it does nothing but emit. The departure itself — the engaged enemies
/// that ride along, the location assignment, the `InvestigatorMoved` event —
/// is [`resolve_departure`], which the timing coordinator runs at
/// `LeftLocation`'s resolve step (#721). The entered half — the destination
/// reveal, auto-engagement, and the entered location's Forced abilities — rides
/// the [`MoveEnter`](crate::state::Continuation::MoveEnter) frame this pushes
/// and runs in [`resume_move_enter`] once the whole departure sequence has
/// resolved (#569).
pub(in crate::engine::dispatch) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    destination: LocationId,
) -> EngineOutcome {
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "move::perform: investigator {investigator:?} absent after the \
                 Status::Active re-validation gate; this is a state-corruption invariant \
                 violation"
            )
        });
    let Some(from) = inv.current_location else {
        // Active but locationless — not expected post-AoO, but suppress
        // (return Done) defensively rather than panic.
        return EngineOutcome::Done;
    };
    if !candidates(cx.state, investigator).contains(&destination) {
        return EngineOutcome::Done; // precondition lapsed: suppress
    }

    // Park the entered-location half on its own frame, then emit in tail
    // position (ADR 0003) with nothing of the departure done yet: since #721
    // `LeftLocation` is coordinator-owned, so the coordinator runs
    // [`resolve_departure`] at its own resolve step, between the `when` and
    // `at` cells. Barricade 01038's *"Forced - When an investigator leaves
    // attached location: Discard Barricade."* therefore discards before the
    // departure lands, which is what the card prints.
    //
    // The frame beneath the emit is #569's: running the engage + entered-
    // location emit inline after the emit — the pre-#569 shape — pushed them
    // above the abilities the emit had just queued, so entering resolved
    // before leaving. The destination reveal rides that frame too; it is the
    // arrival's business, not the departure's.
    cx.state.continuations.push(MoveEnterFrame {
        investigator,
        destination,
    });
    emit::queue_event(
        cx,
        &TimingEvent::LeftLocation {
            investigator,
            location: from,
            destination,
        },
    )
}

/// The departure's own impact: the engaged enemies that ride along (or
/// disengage because they cannot), the investigator's location assignment, and
/// the `InvestigatorMoved` event.
///
/// Since #721 this is `LeftLocation`'s **resolve step** — step 2 of the
/// sequence in `glossary/Nested_Sequences.md` — run by the timing coordinator
/// between the `when` and `at` cells rather than by `perform`
/// before the emit. `from` is the location being left and `destination` the one
/// being entered; both were re-validated by `perform` before it
/// emitted. Reached through
/// [`resolve_left_location`](super::emit::resolve_left_location).
pub(in crate::engine::dispatch) fn resolve_departure(
    cx: &mut Cx,
    investigator: InvestigatorId,
    from: LocationId,
    destination: LocationId,
) {
    if !cx.state.investigators.contains_key(&investigator) {
        // The `when` cell ran between `perform`'s validation and
        // this step, so the actor's presence is re-checked rather than
        // asserted. No corpus card removes an investigator from an interrupt on
        // a departure; suppressing (as the connection re-check above does)
        // beats panicking if one ever does.
        //
        // The rest of the §D re-check — that the actor is still standing at
        // `from`, and that `from` is still connected to a `destination` that
        // still exists — is deliberately *not* repeated here, though the `when`
        // cell can now in principle invalidate all three. Nothing in the corpus
        // relocates an investigator or removes a location from an interrupt on
        // a departure, and a departure that declined to land would still leave
        // the `MoveEnter` frame beneath this sequence to resolve the arrival: a
        // half-move, worse than the state it guards against. The first card
        // that can do either wants the whole move re-validated at this step and
        // the arrival frame cancelled with it — not one field re-read.
        return;
    }
    // Engaged enemies move with the investigator — unless the destination is
    // one the enemy cannot enter. Barricade 01038's "Non-Elite enemies cannot
    // move into attached location" is absolute (RR glossary, "Cannot": "The
    // word 'cannot' is absolute, and cannot be countermanded by other
    // abilities"), so a blocked enemy does not ride along; per the card's
    // ruling (<https://arkhamdb.com/card/01038>), "the engaged enemy will
    // disengage and remain in the investigator's previous location (after
    // making an attack of opportunity)". The AoO has already resolved by the
    // time this runs (#293's `drive_aoo` precedes `perform`).
    //
    // Capture the engagement set before mutating any locations, then update
    // each engaged enemy alongside the investigator's own move.
    //
    // Deliberately *not* through the relocation funnel
    // [`relocate_enemy`](crate::engine::relocate_enemy) (#633): an enemy that follows
    // is already engaged, and `glossary/Enemy_Engagement.md` says such an
    // enemy "remains engaged and moves to the new location simultaneously with
    // the investigator" — there is no engage-on-arrival check to run, and no
    // `EnemyMoved` to emit either (the move is the investigator's,
    // `InvestigatorMoved` below). An enemy that *cannot* follow needs the
    // funnel even less: it never arrives anywhere, staying put at `from` while
    // the investigator leaves. The *unengaged* enemies already standing at the
    // destination are the entered-location half's business, further down.
    let engaged: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in engaged {
        // Ids were collected from this same map with no intervening mutation,
        // so an absent entry is state corruption — surface it the way the
        // elimination path does. The lookup is split in two because the
        // predicate borrows the whole state immutably.
        let enemy = cx.state.enemies.get(&enemy_id).unwrap_or_else(|| {
            unreachable!(
                "resolve_departure: enemy {enemy_id:?} vanished between the engagement scan \
                 and the drag-along; this is a state-corruption invariant violation"
            )
        });
        let follows = movement::enemy_can_enter_location(cx.state, enemy, destination);
        let enemy = cx
            .state
            .enemies
            .get_mut(&enemy_id)
            .expect("presence checked immediately above");
        if follows {
            enemy.current_location = Some(destination);
        } else {
            enemy.engaged_with = None;
            cx.events.push(Event::EnemyDisengaged {
                enemy: enemy_id,
                investigator,
            });
        }
    }
    cx.state
        .investigators
        .get_mut(&investigator)
        .expect("investigator presence re-checked at the top of this function")
        .current_location = Some(destination);
    cx.events.push(Event::InvestigatorMoved {
        investigator,
        from,
        to: destination,
    });
}

/// The entered-location half of a Move, run when the `drive` loop re-exposes the
/// [`MoveEnter`](crate::state::Continuation::MoveEnter) frame — i.e. once the
/// left location's queued `LeftLocation` forced abilities have resolved (#569).
/// Pops the frame, auto-engages, and emits `EnteredLocation` in tail position.
pub(in crate::engine::dispatch) fn resume_move_enter(cx: &mut Cx) -> EngineOutcome {
    let MoveEnterFrame {
        investigator,
        destination,
    } = cx.state.continuations.pop_expect();
    // Reveal the destination if this is the first investigator entry
    // (Rules Reference p.14). No-op if already revealed. Lives here rather than
    // in `perform` because it is the arrival's business: the
    // investigator has to have arrived to have entered, and since #721 the
    // arrival happens at the departure's resolve step, further down the stack.
    reveal::reveal_location(cx, destination);
    // Framework engagement on entering (RR engagement rules): "Each time an
    // investigator enters a location, each ready enemy at that location
    // automatically engages that investigator." Runs after the move is applied
    // and before the entered-location forced window, so an "after you enter"
    // forced ability sees the engagement already established (#496).
    engage_ready_enemies_on_enter(cx, investigator, destination);
    // Terminal step: the entered location's Forced on-enter abilities are queued,
    // and that outcome becomes the move's outcome. This runs *after* the move is
    // applied, so if it returns Rejected, `apply`'s structural rollback restores
    // the pre-move state — the partial mutation above is safe (same reliance on
    // the apply-loop snapshot that `play_card` documents).
    emit::queue_event(
        cx,
        &TimingEvent::EnteredLocation {
            investigator,
            location: destination,
        },
    )
}

/// Auto-engage on entering: every ready (`!exhausted`), currently-unengaged
/// enemy at `location` engages `investigator`, in deterministic `EnemyId` order
/// (RR engagement rules: "Each time an investigator enters a location, each
/// ready enemy at that location automatically engages that investigator.").
///
/// An enemy already engaged with another investigator keeps its current
/// engagement (an enemy engages only one investigator); an enemy that moved here
/// engaged with the entering investigator is skipped (already engaged). No
/// player choice is involved — every qualifying enemy engages the one investigator
/// who entered — so this stays synchronous.
///
/// The **Aloof** carve-out (an Aloof enemy does not automatically engage) is not
/// yet modeled — consistent with the spawn, Hunter, and Upkeep-reengage paths,
/// which also don't gate on Aloof (shared deferral, #144/#150). No Aloof enemy
/// appears in a currently-implemented scenario.
///
/// Scope: this handles the *enter* trigger only. The broader continuous rule
/// (a ready, unengaged enemy co-located with an investigator for any other
/// reason — readied, disengaged, or relocated onto the investigator — engages)
/// is covered for its existing triggers by the Upkeep-readied (#150) and
/// elimination-reengage paths; no other enter-like trigger exists yet.
fn engage_ready_enemies_on_enter(cx: &mut Cx, investigator: InvestigatorId, location: LocationId) {
    let to_engage: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| {
            e.current_location == Some(location) && !e.exhausted && e.engaged_with.is_none()
        })
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in to_engage {
        hunters::engage_enemy_with(cx, enemy_id, investigator);
    }
}
