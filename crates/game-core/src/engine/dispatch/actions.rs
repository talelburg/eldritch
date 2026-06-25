//! Player-action handlers: Investigate, Move, Fight, Evade, plus the
//! engaged-action validation and single-action-spend helpers.

use crate::dsl::SkillTestKind;
use crate::event::Event;
use crate::state::{
    Enemy, EnemyId, GameState, Investigator, InvestigatorId, LocationId, Phase, SkillKind,
    SkillTestFollowUp, Status,
};

use super::super::outcome::EngineOutcome;
use super::Cx;

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
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop. If a
/// cancel/soak window opens the loop suspends; `drive` resumes the
/// frame once the window closes, calling [`investigate_primary_effect`].
///
/// [`Effect::DiscoverClue`]: crate::dsl::Effect::DiscoverClue
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
    cx.state
        .continuations
        .push(crate::state::Continuation::ActionResolution {
            investigator,
            resume: crate::state::ActionResume::Investigate,
        });
    super::combat::drive_aoo(cx, investigator)
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
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "investigate_primary_effect: investigator {investigator:?} not in map after the \
                 Status::Active re-validation gate; this is a state-corruption invariant violation"
            )
        });
    let Some(location_id) = inv.current_location else {
        return EngineOutcome::Done; // lapsed: locationless after AoO
    };
    let Some(location) = cx.state.locations.get(&location_id) else {
        return EngineOutcome::Done;
    };
    if !location.revealed {
        return EngineOutcome::Done; // precondition lapsed
    }
    // Shroud is u8 in state but skill-test difficulty is i8. Saturate
    // at i8::MAX for the absurd case; realistic shrouds are 0–6. The
    // *effective* shroud folds in location-attachment modifiers (Obscuring
    // Fog 01168's +2); fall back to the printed value when no registry is
    // installed (bare unit tests).
    let shroud = match crate::card_registry::current() {
        Some(reg) => crate::engine::evaluator::effective_shroud(reg, location),
        None => location.shroud,
    };
    let difficulty = i8::try_from(shroud).unwrap_or(i8::MAX);
    super::skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        difficulty,
        SkillTestFollowUp::Investigate,
        None,
        None,
        None,
        0, // no weapon/effect modifier on a base Investigate
    )
}

/// Handler for [`PlayerAction::Resource`]. The basic "gain 1 resource"
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
    cx.state
        .continuations
        .push(crate::state::Continuation::ActionResolution {
            investigator,
            resume: crate::state::ActionResume::Resource,
        });
    super::combat::drive_aoo(cx, investigator)
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

/// Handler for [`PlayerAction::Engage`]. Engage an enemy at the
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
    cx.state
        .continuations
        .push(crate::state::Continuation::ActionResolution {
            investigator,
            resume: crate::state::ActionResume::Engage { enemy: enemy_id },
        });
    super::combat::drive_aoo(cx, investigator)
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

/// Handler for [`PlayerAction::Move`].
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

    // Mutate-second. Charge the action (base 1 + surcharge) last — after
    // every move precondition has passed — so a rejected move spends nothing.
    if let Err(rejected) = charge_action(cx, investigator, crate::dsl::ActionClass::Move, "Move") {
        return rejected;
    }

    // Park the move over its attack-of-opportunity loop (#293): push the
    // resume frame, then drive the AoO. If a cancel/soak window opens the loop
    // suspends here; otherwise `drive` resumes the frame and relocates.
    cx.state
        .continuations
        .push(crate::state::Continuation::ActionResolution {
            investigator,
            resume: crate::state::ActionResume::Move { destination },
        });
    super::combat::drive_aoo(cx, investigator)
}

/// The relocation half of a Move, run after its attack-of-opportunity loop
/// completes (#293). Re-derives `from` from the live `current_location` (the `AoO`
/// never moves the actor) and re-checks the destination is still connected —
/// the §D primary-precondition re-check — suppressing the move (returns `Done`)
/// if it no longer holds. Engaged enemies move with the investigator; the
/// entered location's Forced on-enter abilities become the move's outcome.
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
    let still_connected = cx
        .state
        .locations
        .get(&from)
        .is_some_and(|l| l.connections.contains(&destination))
        && cx.state.locations.contains_key(&destination);
    if !still_connected {
        return EngineOutcome::Done; // precondition lapsed: suppress
    }

    // Engaged enemies move with the investigator. Capture the
    // engagement set before mutating any locations, then update each
    // engaged enemy's `current_location` to the destination
    // alongside the investigator's own move.
    let engaged: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| e.engaged_with == Some(investigator))
        .map(|(id, _)| *id)
        .collect();
    cx.state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked above via current_location")
        .current_location = Some(destination);
    for enemy_id in engaged {
        if let Some(enemy) = cx.state.enemies.get_mut(&enemy_id) {
            enemy.current_location = Some(destination);
        }
    }
    cx.events.push(Event::InvestigatorMoved {
        investigator,
        from,
        to: destination,
    });
    // Reveal the destination if this is the first investigator entry
    // (Rules Reference p.14). No-op if already revealed.
    super::reveal::reveal_location(cx, destination);
    // The leaving investigator left `from`: fire any "when an investigator
    // leaves attached location" forced abilities (Barricade 01038 discards
    // itself). In scope this is a single deterministic self-discard, so it
    // resolves synchronously; a 2+-forced suspend here is out of Slice-1 scope
    // (emit_event's loud guard, like the other non-terminal forced sites).
    let left = super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::LeftLocation {
            investigator,
            location: from,
        },
    );
    if !matches!(left, EngineOutcome::Done) {
        return left;
    }
    // Terminal step: the entered location's Forced on-enter abilities fire,
    // and their outcome becomes the move's outcome. This runs *after* the
    // move is applied, so if it returns Rejected (e.g. 2+ simultaneous
    // forced triggers, #213), `apply`'s structural rollback restores the
    // pre-move state — the partial mutation above is safe (same reliance on
    // the apply-loop snapshot that `play_card` documents).
    super::emit::emit_event(
        cx,
        &super::emit::TimingEvent::EnteredLocation {
            investigator,
            location: destination,
        },
    )
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

/// The action-point cost of `action_class` for `investigator`: base 1 plus any
/// Frozen-in-Fear `ExtraActionCost` surcharge (Rules Reference; #164). Pure —
/// reads `card_registry::current()` for the surcharge, falling back to 1 with no
/// registry installed (bare unit tests). The enumerator uses this for Move/Fight/
/// Evade affordability; [`charge_action`] uses it then spends.
pub(crate) fn action_cost(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: crate::dsl::ActionClass,
) -> u8 {
    let extra = match crate::card_registry::current() {
        Some(reg) => {
            crate::engine::evaluator::pending_action_surcharge(
                state,
                reg,
                investigator,
                action_class,
            )
            .0
        }
        None => 0,
    };
    1u8.saturating_add(extra)
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
    action_class: crate::dsl::ActionClass,
    action_name: &str,
) -> Result<(), EngineOutcome> {
    let to_mark = match crate::card_registry::current() {
        Some(reg) => {
            crate::engine::evaluator::pending_action_surcharge(
                cx.state,
                reg,
                investigator,
                action_class,
            )
            .1
        }
        None => Vec::new(),
    };
    let cost = action_cost(cx.state, investigator, action_class);
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

/// Handler for [`PlayerAction::Fight`].
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
    // A `None` location can't host a fight (mirrors `engage`); without it the
    // `enemy.current_location != inv_location` check would let a locationless
    // investigator fight a locationless enemy (`None != None == false`).
    let Some(inv_location) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Fight: {investigator:?} has no current_location to fight from").into(),
        };
    };
    let fight_difficulty = {
        let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
            return EngineOutcome::Rejected {
                reason: format!("Fight: enemy {enemy_id:?} is not in state").into(),
            };
        };
        if enemy.current_location != Some(inv_location) {
            return EngineOutcome::Rejected {
                reason: format!(
                    "Fight: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location ({inv_location:?})",
                    enemy.current_location,
                )
                .into(),
            };
        }
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
    };
    if let Err(rejected) = charge_action(cx, investigator, crate::dsl::ActionClass::Fight, "Fight")
    {
        return rejected;
    }
    super::skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Combat,
        SkillTestKind::Fight,
        fight_difficulty,
        SkillTestFollowUp::Fight {
            enemy: enemy_id,
            extra_damage: 0,
        },
        None,
        None,
        None,
        0, // base Fight: no weapon modifier
    )
}

/// Handler for [`PlayerAction::Evade`].
///
/// Spends 1 action, runs an Agility skill test against the enemy's
/// evade value, and on success disengages and exhausts the enemy.
pub(super) fn evade(cx: &mut Cx, investigator: InvestigatorId, enemy_id: EnemyId) -> EngineOutcome {
    let evade_difficulty = match validate_engaged_action(cx.state, "Evade", investigator, enemy_id)
    {
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
    if let Err(rejected) = charge_action(cx, investigator, crate::dsl::ActionClass::Evade, "Evade")
    {
        return rejected;
    }
    super::skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Agility,
        SkillTestKind::Evade,
        evade_difficulty,
        SkillTestFollowUp::Evade { enemy: enemy_id },
        None,
        None,
        None,
        0, // no weapon/effect modifier on a base Evade
    )
}

#[cfg(test)]
mod actions_tests {
    use crate::action::{Action, PlayerAction};
    use crate::engine::apply;
    use crate::engine::EngineOutcome;
    use crate::event::Event;
    use crate::state::{EnemyId, InvestigatorId, LocationId, Phase, Status};
    use crate::test_support::{
        apply_no_commits, test_enemy, test_investigator, test_location, GameStateBuilder,
    };
    use crate::{assert_event, assert_event_sequence, assert_no_event};

    /// Build a Move scenario: investigator at L1, L1 connected to L2,
    /// 3 actions, Investigation phase, active investigator, with one
    /// engaged ready enemy at the same location.
    fn move_scenario_with_enemy(
        attack_damage: u8,
        inv_health: u8,
    ) -> (
        InvestigatorId,
        LocationId,
        LocationId,
        EnemyId,
        crate::state::GameState,
    ) {
        let inv_id = InvestigatorId(1);
        let l1 = LocationId(10);
        let l2 = LocationId(11);
        let enemy_id = EnemyId(100);

        crate::test_support::install_test_registry();
        let mut inv = test_investigator(1);
        inv.current_location = Some(l1);
        inv.actions_remaining = 3;
        // After #448 cp2a: max_health() reads from the registry (TEST_INV = 8).
        // Pre-load accumulated_damage so the old `inv_health` parameter still
        // determines defeat: total = (8 - inv_health) + attack_damage >= 8
        // ⟺ attack_damage >= inv_health (same condition as before).
        inv.investigator_card.accumulated_damage = 8_u8.saturating_sub(inv_health);

        let mut loc1 = test_location(10, "L1");
        loc1.connections = vec![l2];
        let loc2 = test_location(11, "L2");

        let mut enemy = test_enemy(100, "Ghoul");
        enemy.current_location = Some(l1);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = attack_damage;
        enemy.attack_horror = 0;
        enemy.exhausted = false;

        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc1)
            .with_location(loc2)
            .with_enemy(enemy)
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();

        (inv_id, l1, l2, enemy_id, state)
    }

    #[test]
    fn move_with_lethal_aoo_suppresses_relocation_but_keeps_spent_action() {
        // An engaged enemy whose AoO defeats the investigator: the move is
        // suppressed, the action point + AoO damage persist.
        // Investigator has 1 health, enemy deals 1 damage → lethal AoO.
        //
        // Note: `apply_investigator_defeat` clears `current_location` to `None`
        // on defeat — the investigator is removed from their location as part of
        // defeat resolution. The key invariant is that no `InvestigatorMoved`
        // event fires and the investigator does NOT appear at the destination.
        let (inv_id, _l1, l2, _enemy_id, state) = move_scenario_with_enemy(1, 1);

        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: l2,
            }),
        );

        assert_eq!(result.outcome, crate::engine::EngineOutcome::Done);
        // Action was still spent.
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        // Move is suppressed — investigator did NOT reach L2.
        assert_ne!(
            result.state.investigators[&inv_id].current_location,
            Some(l2),
            "move suppressed: investigator must not appear at destination"
        );
        assert_eq!(
            result.state.investigators[&inv_id].actions_remaining, 2,
            "action still spent"
        );
        // Investigator is no longer Active (defeated by AoO).
        assert_ne!(
            result.state.investigators[&inv_id].status,
            crate::state::Status::Active,
            "investigator not Active after lethal AoO"
        );
        // No InvestigatorMoved emitted.
        assert_no_event!(result.events, Event::InvestigatorMoved { .. });
    }

    #[test]
    fn move_with_nonlethal_aoo_relocates_after_the_attack() {
        // Engaged enemy, 1 damage, investigator survives (8 health):
        // AoO deals damage, then the move resolves.
        // No registry installed → no cancel/soak windows → no suspension.
        let (inv_id, _l1, l2, enemy_id, state) = move_scenario_with_enemy(1, 8);

        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: l2,
            }),
        );

        assert_eq!(result.outcome, crate::engine::EngineOutcome::Done);
        // AoO damage landed.
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 1 }
                if *investigator == inv_id
        );
        // Move proceeded: investigator is at L2.
        assert_eq!(
            result.state.investigators[&inv_id].current_location,
            Some(l2),
            "investigator must have relocated to L2"
        );
        assert_event!(
            result.events,
            Event::InvestigatorMoved { investigator, from: _, to }
                if *investigator == inv_id && *to == l2
        );
        // AoO damage is visible.
        assert_eq!(
            result.state.investigators[&inv_id].damage(),
            1,
            "investigator damage == 1 after nonlethal AoO"
        );
        // Engaged enemy moved with investigator to L2.
        assert_eq!(
            result.state.enemies[&enemy_id].current_location,
            Some(l2),
            "engaged enemy must follow to L2"
        );
        // AoO does not exhaust the attacker (RR p.7).
        assert!(!result.state.enemies[&enemy_id].exhausted);
    }

    /// Build an Investigate scenario: investigator at a revealed location with
    /// 2 clues and shroud 2, 3 actions, Investigation phase, active. Adds a
    /// ready engaged enemy with the given `attack_damage` and a chaos bag
    /// with a single `Numeric(0)` token (intellect 3 vs. shroud 2 → success).
    fn investigate_scenario_with_enemy(
        inv_health: u8,
        attack_damage: u8,
    ) -> (InvestigatorId, LocationId, EnemyId, crate::state::GameState) {
        // Registry needed for max_health()/max_sanity() after cp2a.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let enemy_id = EnemyId(200);

        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        inv.actions_remaining = 3;
        // Pre-load accumulated_damage so that max_health() (8 from TEST_INV) minus
        // accumulated_damage equals inv_health (the "remaining health" the test intended).
        inv.investigator_card.accumulated_damage = 8_u8.saturating_sub(inv_health);

        let mut loc = test_location(10, "Study");
        loc.clues = 2;
        loc.shroud = 2;

        let mut enemy = test_enemy(200, "Ghoul");
        enemy.current_location = Some(loc_id);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = attack_damage;
        enemy.attack_horror = 0;
        enemy.exhausted = false;

        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .with_enemy(enemy)
            .with_chaos_bag(crate::state::ChaosBag::new([
                crate::state::ChaosToken::Numeric(0),
            ]))
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();

        (inv_id, loc_id, enemy_id, state)
    }

    #[test]
    fn investigate_with_nonlethal_aoo_starts_the_test_after_the_attack() {
        // Engaged enemy deals 1 damage, investigator has 8 health (survives).
        // After the AoO, the Investigate skill test starts (AwaitingInput at
        // the commit window). Assert DamageTaken precedes the test start,
        // the investigator is still Active, and took 1 damage.
        let (inv_id, _loc_id, enemy_id, state) = investigate_scenario_with_enemy(8, 1);

        let outcome = apply(
            state,
            crate::action::Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        // Outcome should be AwaitingInput (skill-test commit window).
        assert!(
            matches!(outcome.outcome, EngineOutcome::AwaitingInput { .. }),
            "expected AwaitingInput (commit window) after nonlethal AoO, got {:?}",
            outcome.outcome
        );
        // AoO damage landed before the test started — prove ordering.
        assert_event_sequence!(
            outcome.events,
            Event::DamageTaken { .. },
            Event::SkillTestStarted { .. }
        );
        // Investigator is still Active.
        assert_eq!(
            outcome.state.investigators[&inv_id].status,
            Status::Active,
            "investigator must still be Active after nonlethal AoO"
        );
        // Investigator took 1 damage.
        assert_eq!(
            outcome.state.investigators[&inv_id].damage(),
            1,
            "investigator must have taken 1 damage from AoO"
        );
        // AoO does not exhaust the attacker (RR p.7).
        assert!(!outcome.state.enemies[&enemy_id].exhausted);
    }

    #[test]
    fn investigate_with_lethal_aoo_suppresses_the_test() {
        // AoO deals 1 damage, investigator has 1 health → lethal → no skill
        // test starts, outcome is Done, action was spent, investigator not Active.
        let (inv_id, _loc_id, _enemy_id, state) = investigate_scenario_with_enemy(1, 1);

        let result = apply_no_commits(
            state,
            crate::action::Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        // Outcome is Done (skill test suppressed).
        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done when AoO is lethal, got {:?}",
            result.outcome
        );
        // Action was spent.
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        // No skill test started.
        assert_no_event!(result.events, Event::SkillTestStarted { .. });
        // Investigator is not Active (defeated by AoO).
        assert_ne!(
            result.state.investigators[&inv_id].status,
            Status::Active,
            "investigator must not be Active after lethal AoO"
        );
    }

    /// Build a Resource scenario: investigator at a location, 3 actions,
    /// Investigation phase, active. Adds a ready engaged enemy with the
    /// given `attack_damage` and `inv_health`.
    fn resource_scenario_with_enemy(
        attack_damage: u8,
        inv_health: u8,
    ) -> (InvestigatorId, EnemyId, crate::state::GameState) {
        // Registry needed for max_health()/max_sanity() after cp2a.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let enemy_id = EnemyId(300);

        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        inv.actions_remaining = 3;
        // Pre-load accumulated_damage so that max_health() (8 from TEST_INV) minus
        // accumulated_damage equals inv_health (the "remaining health" the test intended).
        inv.investigator_card.accumulated_damage = 8_u8.saturating_sub(inv_health);
        inv.resources = 0;

        let loc = test_location(10, "Study");

        let mut enemy = test_enemy(300, "Ghoul");
        enemy.current_location = Some(loc_id);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = attack_damage;
        enemy.attack_horror = 0;
        enemy.exhausted = false;

        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .with_enemy(enemy)
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();

        (inv_id, enemy_id, state)
    }

    #[test]
    fn resource_with_lethal_aoo_suppresses_the_gain() {
        // AoO defeats the investigator: no ResourcesGained, action spent.
        // Investigator has 1 health, enemy deals 1 damage → lethal AoO.
        let (inv_id, _enemy_id, state) = resource_scenario_with_enemy(1, 1);

        let result = apply(
            state,
            Action::Player(PlayerAction::Resource {
                investigator: inv_id,
            }),
        );

        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done when AoO is lethal, got {:?}",
            result.outcome
        );
        // Action was still spent.
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        // No resources were gained.
        assert_no_event!(result.events, Event::ResourcesGained { .. });
        // Investigator's resource count is unchanged (still 0).
        assert_eq!(
            result.state.investigators[&inv_id].resources, 0,
            "resources must not change when AoO is lethal"
        );
        // Investigator is no longer Active (defeated by AoO).
        assert_ne!(
            result.state.investigators[&inv_id].status,
            Status::Active,
            "investigator must not be Active after lethal AoO"
        );
    }

    #[test]
    fn resource_with_nonlethal_aoo_still_gains() {
        // Engaged enemy deals 1 damage, investigator has 8 health (survives).
        // After the AoO the Resource gain resolves: ResourcesGained IS emitted,
        // resources incremented by 1, investigator still Active, enemy not
        // exhausted (RR p.7), investigator took 1 damage.
        let (inv_id, enemy_id, state) = resource_scenario_with_enemy(1, 8);

        let result = apply(
            state,
            Action::Player(PlayerAction::Resource {
                investigator: inv_id,
            }),
        );

        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done after nonlethal AoO + resource gain, got {:?}",
            result.outcome
        );
        // ResourcesGained IS emitted (primary effect ran after the AoO).
        assert_event!(
            result.events,
            Event::ResourcesGained { investigator, amount: 1 }
                if *investigator == inv_id
        );
        // Resource count incremented by 1.
        assert_eq!(
            result.state.investigators[&inv_id].resources, 1,
            "resources must increase by 1 after a nonlethal AoO"
        );
        // Investigator is still Active.
        assert_eq!(
            result.state.investigators[&inv_id].status,
            Status::Active,
            "investigator must still be Active after nonlethal AoO"
        );
        // AoO does not exhaust the attacker (RR p.7).
        assert!(
            !result.state.enemies[&enemy_id].exhausted,
            "AoO must not exhaust the attacker (RR p.7)"
        );
        // Investigator took the AoO damage.
        assert_eq!(
            result.state.investigators[&inv_id].damage(),
            1,
            "investigator damage == 1 after nonlethal AoO"
        );
    }

    #[test]
    fn resource_with_no_engaged_enemy_gains_normally() {
        // No engaged enemy: behaviour-preserving — resources +1, Done.
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);

        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        inv.actions_remaining = 3;
        inv.resources = 2;

        let loc = test_location(10, "Study");

        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();

        let result = apply(
            state,
            Action::Player(PlayerAction::Resource {
                investigator: inv_id,
            }),
        );

        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done on no-AoO resource gain, got {:?}",
            result.outcome
        );
        // Action was spent.
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        // ResourcesGained emitted.
        assert_event!(
            result.events,
            Event::ResourcesGained { investigator, amount: 1 }
                if *investigator == inv_id
        );
        // Resource count incremented.
        assert_eq!(
            result.state.investigators[&inv_id].resources, 3,
            "resources must increase by 1"
        );
    }

    /// Build an Engage scenario: investigator at L1, a target enemy at L1
    /// (not yet engaged), and an `AoO` enemy at L1 already engaged with the
    /// investigator. Returns `(inv_id, target_id, aoo_enemy_id, state)`.
    fn engage_scenario_with_aoo_enemy(
        inv_health: u8,
        aoo_damage: u8,
    ) -> (InvestigatorId, EnemyId, EnemyId, crate::state::GameState) {
        // Registry needed for max_health()/max_sanity() after cp2a.
        crate::test_support::install_test_registry();
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let target_id = EnemyId(400);
        let aoo_enemy_id = EnemyId(401);

        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        inv.actions_remaining = 3;
        // Pre-load accumulated_damage so that max_health() (8 from TEST_INV) minus
        // accumulated_damage equals inv_health (the "remaining health" the test intended).
        inv.investigator_card.accumulated_damage = 8_u8.saturating_sub(inv_health);

        let loc = test_location(10, "Study");

        // The target: co-located, not yet engaged — cannot AoO.
        let mut target = test_enemy(400, "Cultist");
        target.current_location = Some(loc_id);
        target.engaged_with = None;
        target.attack_damage = 0;
        target.attack_horror = 0;
        target.exhausted = false;

        // The AoO attacker: already engaged, ready — it WILL AoO.
        let mut aoo_enemy = test_enemy(401, "Ghoul");
        aoo_enemy.current_location = Some(loc_id);
        aoo_enemy.engaged_with = Some(inv_id);
        aoo_enemy.attack_damage = aoo_damage;
        aoo_enemy.attack_horror = 0;
        aoo_enemy.exhausted = false;

        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .with_enemy(target)
            .with_enemy(aoo_enemy)
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();

        (inv_id, target_id, aoo_enemy_id, state)
    }

    #[test]
    fn engage_with_nonlethal_aoo_engages_after_the_attack() {
        // A second engaged enemy AoOs (1 damage); the target (co-located, not yet
        // engaged) is then engaged. Assert DamageTaken (the AoO) precedes EnemyEngaged, the
        // target's engaged_with == Some(investigator), investigator survived.
        let (inv_id, target_id, aoo_enemy_id, state) = engage_scenario_with_aoo_enemy(8, 1);

        let result = apply(
            state,
            Action::Player(PlayerAction::Engage {
                investigator: inv_id,
                enemy: target_id,
            }),
        );

        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done after nonlethal AoO + engage, got {:?}",
            result.outcome
        );
        // AoO damage landed.
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 1 }
                if *investigator == inv_id
        );
        // EnemyEngaged fired (target is now engaged).
        assert_event!(
            result.events,
            Event::EnemyEngaged { enemy, investigator }
                if *enemy == target_id && *investigator == inv_id
        );
        // AoO damage (DamageTaken) precedes the engagement.
        assert_event_sequence!(
            result.events,
            Event::DamageTaken { .. },
            Event::EnemyEngaged { .. }
        );
        // Target is now engaged with the investigator.
        assert_eq!(
            result.state.enemies[&target_id].engaged_with,
            Some(inv_id),
            "target must be engaged with the investigator after engage action"
        );
        // Investigator is still Active.
        assert_eq!(
            result.state.investigators[&inv_id].status,
            crate::state::Status::Active,
            "investigator must still be Active after nonlethal AoO"
        );
        // Investigator took 1 damage.
        assert_eq!(
            result.state.investigators[&inv_id].damage(),
            1,
            "investigator damage == 1 after nonlethal AoO"
        );
        // AoO attacker is not exhausted (RR p.7).
        assert!(!result.state.enemies[&aoo_enemy_id].exhausted);
    }

    #[test]
    fn engage_with_lethal_aoo_suppresses_the_engagement() {
        // The other engaged enemy's AoO defeats the investigator: no EnemyEngaged.
        let (inv_id, target_id, _aoo_enemy_id, state) = engage_scenario_with_aoo_enemy(1, 1);

        let result = apply(
            state,
            Action::Player(PlayerAction::Engage {
                investigator: inv_id,
                enemy: target_id,
            }),
        );

        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "expected Done when AoO is lethal, got {:?}",
            result.outcome
        );
        // Action was still spent.
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        // No engagement — target must not be engaged.
        assert_no_event!(result.events, Event::EnemyEngaged { .. });
        assert_eq!(
            result.state.enemies[&target_id].engaged_with, None,
            "target must not be engaged when AoO is lethal"
        );
        // Investigator is not Active (defeated by AoO).
        assert_ne!(
            result.state.investigators[&inv_id].status,
            crate::state::Status::Active,
            "investigator must not be Active after lethal AoO"
        );
    }
}
