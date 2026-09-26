//! Player-action handlers: Investigate, Move, Fight, Evade, plus the
//! engaged-action validation and single-action-spend helpers.

use card_dsl::dsl::{ActionClass, IntExpr, SkillTestKind, Stat};

use crate::card_registry;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::skill_test::InitiatorModifier;
use crate::engine::dispatch::{combat, emit, hunters, movement, reveal, skill_test};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{designator, evaluator, Cx};
use crate::event::Event;
use crate::state::{
    AbilitySource, ActionResume, CardInstanceId, Continuation, DifficultyBasis, Enemy, EnemyId,
    GameState, Investigator, InvestigatorId, LocationId, ModifierTarget, Phase, SkillKind,
    SkillTestFollowUp, Status,
};

/// Handler for `TurnAction::Investigate`.
///
/// Spends 1 action, runs an intellect skill test against the location's
/// shroud, and on success applies [`Effect::DiscoverClue`] to move 1
/// clue from the location to the investigator. The discover-clue
/// evaluator handles the location-empty edge case as a silent no-op,
/// so an investigation at a 0-clue location costs the action and runs
/// the test but yields nothing. That is the printed rule, and in
/// particular **a clueless location is not an illegal target** — no
/// gate here counts clues, deliberately.
/// `data/official-faq/Frequently_Asked_Questions.md`:
///
/// > Q: Can I investigate a location with no clues on it? If I do, what
/// > happens?
/// >
/// > A: Yes. You can investigate a location even if there are no clues on
/// > it. However, you won't be able to discover any clues there, because
/// > there are no clues on the location to discover. Investigating a
/// > location with no clues might still be useful to trigger card
/// > abilities such as Burglary (\[core\] 45) or Scavenging (\[core\] 73).
///
/// The last sentence is why the "no potential to change the game state"
/// initiation gate (`glossary/Ability.md`) must not be read as a clue
/// check on the basic action: the test itself is the state change other
/// cards key off.
///
/// Card-derived investigate variants (Rite of Seeking's "Action:
/// Investigate using willpower instead of intellect", Working a
/// Hunch's discover-without-test) implement their own paths; this
/// handler is the bare turn-action.
///
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop. If a
/// cancel/soak window opens the loop suspends; `drive` resumes the
/// frame once the window closes, calling [`investigate_primary_effect`].
///
/// [`Effect::DiscoverClue`]: card_dsl::dsl::Effect::DiscoverClue
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(super) fn investigate(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    // Validate-first (the shared basic-action prefix, then the
    // location-specific checks).
    let inv = match validate_basic_action(cx.state, "Investigate", investigator) {
        Ok(inv) => inv,
        Err(rejection) => return rejection,
    };
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
    let location = cx.state.locations.get(&location_id).unwrap_or_else(|| {
        unreachable!(
            "Investigate: location {location_id:?} (investigator's current_location) \
             is not in the locations map; this is a state-corruption invariant violation"
        )
    });
    if !location.revealed {
        return EngineOutcome::Rejected {
            reason: format!("Investigate: location {location_id:?} is not revealed").into(),
        };
    }

    // Mutate-second: spend the action, then park the investigate over
    // its attack-of-opportunity loop (#293). Push the resume frame,
    // then drive the AoO. Investigate is NOT on the AoO-exempt list
    // (only Fight, Evade, Parley, Resign are), so each ready engaged
    // enemy attacks before the skill test resolves.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(Continuation::ActionResolution {
        investigator,
        resume: ActionResume::Investigate,
    });
    combat::drive_aoo(cx, investigator)
}

/// The skill-test half of an Investigate, run after its `AoO` loop (#293).
/// Re-reads the location + effective shroud live and re-checks the location
/// is still revealed (the §D precondition re-check); suppresses (returns
/// `Done`) if the precondition has lapsed.
///
/// A missing investigator map entry panics — `resume_action_resolution`'s
/// `Status::Active` gate upstream already guarantees the investigator is
/// present, so absence here is a state-corruption invariant violation. A
/// legitimately lapsed precondition (no `current_location`, or location
/// absent / not `revealed`) returns `Done` instead.
pub(super) fn investigate_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    assert!(
        cx.state.investigators.contains_key(&investigator),
        "investigate_primary_effect: investigator {investigator:?} not in map after the \
         Status::Active re-validation gate; this is a state-corruption invariant violation"
    );
    // Locationless after the AoO, or the location gone / no longer revealed:
    // the precondition lapsed, so suppress the primary rather than rejecting
    // (the §D contract). Read through the same helper `can_perform` uses, so
    // the basic action and a designated **Investigate** agree on what a
    // location has to be to investigate it (#805).
    let Some(location_id) = designator::investigate_location(cx.state, investigator) else {
        return EngineOutcome::Done;
    };
    // A basic investigation carries no modification — the designated one
    // (Flashlight 01087) reaches the same primary with its `-2 [shroud]`.
    perform_investigate(cx, investigator, location_id, None, None)
}

/// Perform an **investigate** against `location_id`: an Intellect test whose
/// difficulty *is* that location's modified shroud, read live at ST.6 rather
/// than snapshotted here (#677), with the base Investigate follow-up (so a
/// success discovers a clue).
///
/// The one primary behind both ways of investigating (#805) — the basic action
/// (via [`investigate_primary_effect`], after its attack-of-opportunity loop)
/// and an ability printing the bold **Investigate** designator (Flashlight
/// 01087). `glossary/Ability.md` is what makes them the same procedure:
/// *"Activating such an ability **performs the designated action** as described
/// in the rules, but modified in the manner described by the ability."* The
/// modification is `shroud_modifier`, and it is the *only* thing that differs.
///
/// `shroud_modifier` adjusts the **location difficulty** (shroud), not the
/// investigator's total, and it is one contribution among however many the
/// location carries: Flashlight's `-2` composes with Obscuring Fog 01168's `+2`
/// into a single shroud, clamped at 0 once at the end (RR p.4: game values can
/// never be reduced below 0). It travels into the test unevaluated so the row
/// it becomes is recalculated at every read (ADR 0005).
///
/// Callers validate that the location exists and is revealed; this takes the id
/// as given.
pub(crate) fn perform_investigate(
    cx: &mut Cx,
    investigator: InvestigatorId,
    location_id: LocationId,
    shroud_modifier: Option<IntExpr>,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        DifficultyBasis::Shroud(location_id),
        SkillTestFollowUp::Investigate,
        None,
        None,
        source,
        // "Your location gets -2 shroud for this investigation" — a row over
        // the *location*, scoped to the test about to start.
        shroud_modifier.map(|delta| InitiatorModifier {
            target: ModifierTarget::Location(location_id),
            stat: Stat::Shroud,
            delta,
        }),
    )
}

/// Handler for `TurnAction::Resource`. The basic "gain 1 resource"
/// action (Rules Reference, Investigation step 2.2.1).
///
/// Validate-first: Investigation phase, `investigator` is active and
/// `Status::Active`, `actions_remaining >= 1`. Mutate-second: spend 1
/// action, push an [`ActionResolution`] frame, and drive the
/// attack-of-opportunity loop (#293). If the investigator survives the
/// `AoO` loop, [`resource_primary_effect`] fires and gains 1 resource.
///
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(super) fn resource_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if let Err(rejection) = validate_basic_action(cx.state, "Resource", investigator) {
        return rejection;
    }

    // Mutate-second: spend the action, then park the resource gain over its
    // attack-of-opportunity loop (#293). Push the resume frame, then drive
    // the AoO. Resource is NOT on the AoO-exempt list (only Fight, Evade,
    // Parley, Resign are), so each ready engaged enemy attacks before the
    // gain resolves.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(Continuation::ActionResolution {
        investigator,
        resume: ActionResume::Resource,
    });
    combat::drive_aoo(cx, investigator)
}

/// The gain half of a Resource action, run after its `AoO` loop (#293).
///
/// Resource has no target precondition (unlike Move or Investigate), so
/// there is no secondary precondition re-check here. The `resume_action_resolution`
/// `Status::Active` gate upstream already guarantees the investigator is
/// present and Active; a missing map entry here is therefore a
/// state-corruption invariant violation — it must `unreachable!`-panic.
/// There is no legitimate `Done`-return inside `resource_primary_effect`:
/// it always gains 1 resource and returns `Done`.
pub(super) fn resource_primary_effect(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "resource_primary_effect: investigator {investigator:?} not in map after the \
                 Status::Active re-validation gate; this is a state-corruption invariant violation"
            )
        });
    inv_mut.resources = inv_mut.resources.saturating_add(1);
    cx.events.push(Event::ResourcesGained {
        investigator,
        amount: 1,
    });
    EngineOutcome::Done
}

/// Handler for `TurnAction::Engage`. Engage an enemy at the
/// investigator's location that they are not already engaged with
/// (Rules Reference p.4) — it becomes engaged with the investigator.
///
/// Validate-first: Investigation phase, active + `Status::Active`,
/// `actions_remaining >= 1`, enemy in state, enemy at the investigator's
/// `current_location`, not already engaged with the investigator.
/// Mutate-second: spend 1 action, then park the engagement over its
/// attack-of-opportunity loop (#293). The target enemy is not yet engaged
/// so it cannot `AoO`; only OTHER ready engaged enemies do. If the
/// investigator survives, [`engage_primary_effect`] runs the engagement.
///
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop.
///
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(super) fn engage(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let inv = match validate_basic_action(cx.state, "Engage", investigator) {
        Ok(inv) => inv,
        Err(rejection) => return rejection,
    };
    // A `None` location can't host an engage (matches `investigate`'s
    // guard); without it the `enemy.current_location != inv_location`
    // check below would let a locationless investigator engage a
    // locationless enemy (`None != None == false`).
    let Some(inv_location) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} has no current_location to engage from")
                .into(),
        };
    };
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Rejected {
            reason: format!("Engage: enemy {enemy_id:?} is not in state").into(),
        };
    };
    if enemy.engaged_with == Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} is already engaged with {enemy_id:?}").into(),
        };
    }
    if enemy.current_location != Some(inv_location) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Engage: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location ({inv_location:?})",
                enemy.current_location,
            )
            .into(),
        };
    }

    // Mutate-second: spend the action, then park the engagement over its
    // attack-of-opportunity loop (#293). Push the resume frame, then drive
    // the AoO. Engage is NOT on the AoO-exempt list (only Fight, Evade,
    // Parley, Resign are). The target is not yet engaged so it cannot AoO;
    // only OTHER ready engaged enemies do.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(Continuation::ActionResolution {
        investigator,
        resume: ActionResume::Engage { enemy: enemy_id },
    });
    combat::drive_aoo(cx, investigator)
}

/// The engagement half of an Engage action, run after its `AoO` loop (#293).
///
/// Re-reads the enemy from live state and re-checks the target precondition
/// (the §D primary-precondition re-check): enemy still exists, is co-located
/// with the investigator, and is not already engaged with this investigator.
/// Returns `Done` on any lapsed precondition (the engagement simply does not
/// happen — legitimately suppressed, not a state corruption).
///
/// A missing investigator map entry after the `Status::Active` gate in
/// `resume_action_resolution` is a state-corruption invariant violation and
/// must `unreachable!`-panic — absence here is impossible if the gate held.
pub(super) fn engage_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "engage_primary_effect: investigator {investigator:?} not in map after the \
                 Status::Active re-validation gate; this is a state-corruption invariant violation"
            )
        });
    let Some(inv_location) = inv.current_location else {
        return EngineOutcome::Done; // lapsed: investigator lost its location during the AoO
    };
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Done; // lapsed: target gone
    };
    if enemy.engaged_with == Some(investigator) || enemy.current_location != Some(inv_location) {
        return EngineOutcome::Done; // lapsed: already engaged, or no longer co-located
    }
    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).expect("checked above");
    enemy_mut.engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator,
    });
    EngineOutcome::Done
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
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop. If a
/// cancel/soak window opens the loop suspends; `drive` resumes the
/// frame once the window closes, calling [`move_primary_effect`].
///
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(super) fn move_action(
    cx: &mut Cx,
    investigator: InvestigatorId,
    destination: LocationId,
) -> EngineOutcome {
    // Validate-first.
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Move is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Move: {investigator:?} is not the active investigator ({:?})",
                cx.state.active_investigator,
            )
            .into(),
        };
    }
    // Active-investigator + missing-from-map is a state-corruption
    // invariant violation (active_investigator is engine-set; the
    // pairing with the map entry is an invariant), so surface loudly.
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
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
    let from_loc = cx.state.locations.get(&from).unwrap_or_else(|| {
        unreachable!(
            "Move: location {from:?} (investigator's current_location) is not in the \
             locations map; this is a state-corruption invariant violation"
        )
    });
    if !cx.state.locations.contains_key(&destination) {
        return EngineOutcome::Rejected {
            reason: format!("Move: destination {destination:?} is not in state").into(),
        };
    }
    if !from_loc.connections.contains(&destination) {
        return EngineOutcome::Rejected {
            reason: format!("Move: {destination:?} is not connected to {from:?}").into(),
        };
    }
    // The movement barrier (#774). Checked here as well as in `legal_actions`
    // because the `apply` seam is submittable directly: a client that never
    // read the menu, or read a stale one, must still be refused. The Parlor
    // 01115's unrevealed back is the only card in the corpus that prints one.
    if !movement::investigator_can_enter_location(cx.state, destination) {
        return EngineOutcome::Rejected {
            reason: format!("Move: movement into {destination:?} is blocked by a card ability")
                .into(),
        };
    }

    // Mutate-second. Charge the action (base 1 + surcharge) last — after
    // every move precondition has passed — so a rejected move spends nothing.
    if let Err(rejected) = charge_action(cx, investigator, ActionClass::Move, "Move") {
        return rejected;
    }

    // Park the move over its attack-of-opportunity loop (#293): push the
    // resume frame, then drive the AoO. If a cancel/soak window opens the loop
    // suspends here; otherwise `drive` resumes the frame and relocates.
    cx.state.continuations.push(Continuation::ActionResolution {
        investigator,
        resume: ActionResume::Move { destination },
    });
    combat::drive_aoo(cx, investigator)
}

/// The relocation half of a Move, run after its attack-of-opportunity loop
/// completes (#293). Re-derives `from` from the live `current_location` (the `AoO`
/// never moves the actor) and re-checks the destination is still connected and
/// still enterable — the §D primary-precondition re-check — suppressing the
/// move (returns `Done`) if either no longer holds. The barrier re-check is
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
pub(super) fn move_primary_effect(
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
                "move_primary_effect: investigator {investigator:?} absent after the \
                 Status::Active re-validation gate; this is a state-corruption invariant \
                 violation"
            )
        });
    let Some(from) = inv.current_location else {
        // Active but locationless — not expected post-AoO, but suppress
        // (return Done) defensively rather than panic.
        return EngineOutcome::Done;
    };
    let still_enterable = cx
        .state
        .locations
        .get(&from)
        .is_some_and(|l| l.connections.contains(&destination))
        && cx.state.locations.contains_key(&destination)
        && movement::investigator_can_enter_location(cx.state, destination);
    if !still_enterable {
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
    cx.state.continuations.push(Continuation::MoveEnter {
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
/// between the `when` and `at` cells rather than by `move_primary_effect`
/// before the emit. `from` is the location being left and `destination` the one
/// being entered; both were re-validated by `move_primary_effect` before it
/// emitted. Reached through
/// [`resolve_left_location`](super::emit::resolve_left_location).
pub(super) fn resolve_departure(
    cx: &mut Cx,
    investigator: InvestigatorId,
    from: LocationId,
    destination: LocationId,
) {
    if !cx.state.investigators.contains_key(&investigator) {
        // The `when` cell ran between `move_primary_effect`'s validation and
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
    // time this runs (#293's `drive_aoo` precedes `move_primary_effect`).
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
pub(super) fn resume_move_enter(cx: &mut Cx) -> EngineOutcome {
    let Some(Continuation::MoveEnter {
        investigator,
        destination,
    }) = cx.state.continuations.pop()
    else {
        unreachable!("resume_move_enter: top frame is not a MoveEnter");
    };
    // Reveal the destination if this is the first investigator entry
    // (Rules Reference p.14). No-op if already revealed. Lives here rather than
    // in `move_primary_effect` because it is the arrival's business: the
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

/// Validate the preconditions shared by every action-point-spending
/// basic action: Investigation phase, `investigator` is the active
/// investigator, `Status::Active`, and at least one action remaining.
/// Returns the validated investigator. `action_name` is interpolated
/// into rejection reasons; an active investigator missing from the map
/// is a state-corruption invariant and panics.
///
/// Move / Fight / Evade defer the action-point check to `charge_action`
/// (which folds in the Frozen-in-Fear surcharge), so `move_action` keeps
/// its own prefix; `fight` calls this directly then does its own co-location
/// check (#401), while `evade` reaches it via [`validate_engaged_action`],
/// which adds the engagement check.
pub(crate) fn validate_basic_action<'a>(
    state: &'a GameState,
    action_name: &'static str,
    investigator: InvestigatorId,
) -> Result<&'a Investigator, EngineOutcome> {
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
    Ok(inv)
}

/// Validate the Evade prefix: the basic-action preconditions (via
/// [`validate_basic_action`]) plus enemy exists and is engaged with the
/// named enemy. Returns the borrowed enemy so the caller can read the evade
/// difficulty and any other fields it needs. (Only `evade` uses this — Evade
/// is engagement-only per RR p.11; `fight` is co-location-gated since #401 and
/// does its own check.)
///
/// On `Err`, returns the rejection; the caller should propagate it
/// without further state mutation. State-corruption invariants
/// (active investigator missing from map) panic via `unreachable!`.
///
/// Does NOT validate the evade difficulty is non-negative — the caller does
/// that after the engagement check, so a malformed `evade: -1` rejects with a
/// clear reason rather than being silently clamped.
fn validate_engaged_action<'a>(
    state: &'a GameState,
    action_name: &'static str,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> Result<&'a Enemy, EngineOutcome> {
    validate_basic_action(state, action_name, investigator)?;
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

/// Validate that `enemy_id` is a legal Fight target for `investigator`: it is
/// one of the enemies a Fight may target (RR p.12, *"To fight an enemy **at his
/// or her location**…"* — engagement is not required, unlike Evade), and its
/// printed fight value is not malformed.
///
/// Candidacy is
/// [`designator::fight_candidates`](crate::engine::designator::fight_candidates)
/// — the same list a designated **Fight** grounds its pick against and the same
/// one `can_perform` counts pre-cost (#805). The basic action differs only in
/// naming its target up front instead of choosing among them, which is why it
/// reads the *list* rather than `can_perform` itself: *"is **this** enemy a
/// legal target"* is a question the activation gate deliberately does not ask
/// (it asks only whether **some** target exists, and leaves the pick to the
/// evaluator).
///
/// Returns nothing on success: the fight value it range-checks is read
/// again at ST.6 through the modified-value query, not carried out of
/// here (#677).
fn validate_fight_target(
    state: &GameState,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> Result<(), EngineOutcome> {
    let Some(enemy) = state.enemies.get(&enemy_id) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("Fight: enemy {enemy_id:?} is not in state").into(),
        });
    };
    if !designator::fight_candidates(state, investigator).contains(&enemy_id) {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "Fight: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location",
                enemy.current_location,
            )
            .into(),
        });
    }
    if enemy.fight < 0 {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "Fight: enemy {enemy_id:?} has negative fight value {} (malformed state)",
                enemy.fight,
            )
            .into(),
        });
    }
    Ok(())
}

/// Spend 1 action point from the active investigator and emit
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= 1`.
pub(super) fn spend_one_action(cx: &mut Cx, investigator: InvestigatorId) {
    spend_actions(cx, investigator, 1);
}

/// Spend `n` action points from the active investigator and emit a single
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= n`.
pub(super) fn spend_actions(cx: &mut Cx, investigator: InvestigatorId, n: u8) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked before spend_actions");
    let new_count = inv.actions_remaining - n;
    inv.actions_remaining = new_count;
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

/// What one basic action costs before any surcharge. Named so the surcharge's
/// two consumers — [`action_cost`] and [`charge_action`] — state the formula
/// once between them.
const BASIC_ACTION_COST: u8 = 1;

/// The `ExtraActionCost` surcharge on `action_class` for `investigator`, plus
/// the `first_each_round` sources to mark spent once the action commits.
///
/// The registry-optional wrapper around
/// [`pending_action_surcharge`](crate::engine::evaluator::pending_action_surcharge):
/// no registry (bare unit tests) means no card data, so no surcharge. Pure, so
/// a caller can peek at the cost before deciding to pay it — which is what
/// validate-first requires of both consumers.
///
/// **Both ways of taking an action of a class read this**: the basic-action
/// handlers via [`action_cost`] / [`charge_action`], and an activated ability
/// whose bold designator names the class via
/// [`ActionDesignator::action_class`](card_dsl::dsl::ActionDesignator::action_class)
/// (#754). Sharing it is the point — a surcharge only one of the two applies is
/// the bug that made shooting a weapon cheaper than punching.
pub(crate) fn action_surcharge(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: ActionClass,
) -> (u8, Vec<CardInstanceId>) {
    match card_registry::current() {
        Some(reg) => evaluator::pending_action_surcharge(state, reg, investigator, action_class),
        None => (0, Vec::new()),
    }
}

/// The action-point cost of a **basic** `action_class` for `investigator`: base
/// 1 plus any Frozen-in-Fear `ExtraActionCost` surcharge (Rules Reference;
/// #164). Pure. The enumerator uses this for Move/Fight/Evade affordability;
/// [`charge_action`] uses it then spends.
///
/// An activated ability pays its *printed* action cost plus the same surcharge
/// rather than this, since the printed cost need not be 1 — see
/// `check_activate_ability`.
pub(crate) fn action_cost(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: ActionClass,
) -> u8 {
    BASIC_ACTION_COST.saturating_add(action_surcharge(state, investigator, action_class).0)
}

/// Charge the action cost for `action_class` (base 1 + any Frozen-in-Fear
/// `ExtraActionCost` surcharge): validate-first, returning `Err(Rejected)`
/// without mutating if the investigator lacks the points. On `Ok` the
/// actions are spent and the surcharge sources are marked spent for the
/// round. **Mutates on success**, so call it after every other precondition
/// for the action has passed. Falls back to cost 1 with no surcharge when
/// no registry is installed (bare unit tests). Shared by move/fight/evade.
fn charge_action(
    cx: &mut Cx,
    investigator: InvestigatorId,
    action_class: ActionClass,
    action_name: &str,
) -> Result<(), EngineOutcome> {
    let (extra, to_mark) = action_surcharge(cx.state, investigator, action_class);
    let cost = BASIC_ACTION_COST.saturating_add(extra);
    let remaining = cx
        .state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.actions_remaining);
    if remaining < cost {
        return Err(EngineOutcome::Rejected {
            reason: format!("{action_name} requires {cost} action point(s)").into(),
        });
    }
    spend_actions(cx, investigator, cost);
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.action_surcharge_spent_this_round.extend(to_mark);
    }
    Ok(())
}

/// Handler for `TurnAction::Fight`.
///
/// Spends 1 action, runs a Combat skill test against the enemy's
/// fight value, and on success deals 1 damage. If damage reaches
/// `max_health`, the enemy is defeated and removed from play.
///
/// Per Rules Reference p.12 ("To fight an enemy **at his or her location**…"),
/// Fight targets any enemy at the investigator's location — engaged with them or
/// not (unlike Evade, which is engagement-only; RR p.11). The eligibility check
/// is co-location, mirroring [`engage`] (#401).
///
/// Damage > 1 (weapons, card buffs), after-success / after-failure
/// triggers (#64), and `AoO` from *other* engaged enemies (#78) are all
/// downstream. `AoO` does NOT fire on Fight itself per the Rules
/// Reference's `AoO`-exempt list.
pub(super) fn fight(cx: &mut Cx, investigator: InvestigatorId, enemy_id: EnemyId) -> EngineOutcome {
    let inv = match validate_basic_action(cx.state, "Fight", investigator) {
        Ok(inv) => inv,
        Err(rejection) => return rejection,
    };
    // A `None` location can't host a fight (mirrors `engage`); `fight_candidates`
    // is empty for a locationless investigator, so the target check below would
    // reject anyway — but with a message about the enemy rather than about the
    // investigator standing nowhere.
    if inv.current_location.is_none() {
        return EngineOutcome::Rejected {
            reason: format!("Fight: {investigator:?} has no current_location to fight from").into(),
        };
    }
    if let Err(rejection) = validate_fight_target(cx.state, investigator, enemy_id) {
        return rejection;
    }
    if let Err(rejected) = charge_action(cx, investigator, ActionClass::Fight, "Fight") {
        return rejected;
    }
    // A basic attack carries no modification — a designated Fight (every
    // corpus weapon) reaches the same primary with its combat bonus and its
    // bonus damage.
    perform_fight(cx, investigator, enemy_id, None, 0, None)
}

/// Perform a **fight** against `enemy_id`: a Combat test whose difficulty *is*
/// that enemy's modified fight value, read at ST.6 rather than snapshotted here
/// (#677), dealing `1 + extra_damage` on success.
///
/// The one primary behind both ways of attacking (#805) — the basic Fight
/// action and an ability printing the bold **Fight** designator (every weapon
/// in the corpus). `glossary/Ability.md`: *"Activating such an ability
/// **performs the designated action** as described in the rules, but modified
/// in the manner described by the ability."* The modification is
/// `combat_modifier` + `extra_damage`, and it is the *only* thing that differs;
/// before this the two built their own near-identical tests side by side.
///
/// `combat_modifier` is *"+N \[combat\] for this attack"* — a row over the
/// controller's combat skill scoped to the test about to start, not a number
/// added to a snapshotted total. It travels unevaluated so the row answers from
/// the board at every read (ADR 0005), which is what makes Esoteric Formula
/// 02254's *"+2 \[willpower\] for this attack for each clue on the attacked
/// enemy"* expressible without a second mechanism. `extra_damage` is already a
/// number here: the Fight follow-up consumes it as a `u8`.
///
/// Callers validate the target; this takes the id as given.
pub(crate) fn perform_fight(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    combat_modifier: Option<IntExpr>,
    extra_damage: u8,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Combat,
        SkillTestKind::Fight,
        DifficultyBasis::Fight(enemy_id),
        SkillTestFollowUp::Fight {
            enemy: enemy_id,
            extra_damage,
        },
        None,
        None,
        source,
        combat_modifier.map(|delta| InitiatorModifier {
            target: ModifierTarget::Investigator(investigator),
            stat: Stat::Combat,
            delta,
        }),
    )
}

/// Handler for `TurnAction::Evade`.
///
/// Spends 1 action, runs an Agility skill test against the enemy's
/// evade value, and on success disengages and exhausts the enemy.
pub(super) fn evade(cx: &mut Cx, investigator: InvestigatorId, enemy_id: EnemyId) -> EngineOutcome {
    // The evade value it range-checks is read again at ST.6 through the
    // modified-value query, not carried out of here (#677).
    match validate_engaged_action(cx.state, "Evade", investigator, enemy_id) {
        Ok(enemy) if enemy.evade < 0 => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "Evade: enemy {enemy_id:?} has negative evade value {} (malformed state)",
                    enemy.evade,
                )
                .into(),
            }
        }
        Ok(_) => {}
        Err(rejected) => return rejected,
    }
    if let Err(rejected) = charge_action(cx, investigator, ActionClass::Evade, "Evade") {
        return rejected;
    }
    skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Agility,
        SkillTestKind::Evade,
        // The difficulty *is* the enemy's modified evade value, read at
        // ST.6 — Cold Spring Glen 02244's "Each enemy in Cold Spring Glen
        // gets -1 evade" reaches this test (#677).
        DifficultyBasis::Evade(enemy_id),
        SkillTestFollowUp::Evade { enemy: enemy_id },
        None,
        None,
        None,
        None, // no weapon/effect modifier on a base Evade
    )
}

#[cfg(test)]
mod tests;
