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
/// [`Effect::DiscoverClue`]: crate::dsl::Effect::DiscoverClue
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

    // Mutate-second: spend the action, fire AoO, then resolve the
    // test. Investigate is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Resign are), so each ready engaged enemy attacks
    // before the test resolves.
    spend_one_action(cx, investigator);
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    // If AoO defeated the investigator, the action's primary effect
    // (the skill test) is suppressed. The action point and AoO events
    // already fired — they stay. The action declaration was legal;
    // the investigator just can't complete it.
    let inv_after_aoo = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
            "Investigate: investigator {investigator:?} disappeared between AoO and skill test; \
             this is a state-corruption invariant violation"
        )
        });
    if inv_after_aoo.status != Status::Active {
        return EngineOutcome::Done;
    }

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
/// action, fire attacks of opportunity (Resource is NOT `AoO`-exempt),
/// then — if the investigator survived the `AoO` — gain 1 resource.
pub(super) fn resource_action(cx: &mut Cx, investigator: InvestigatorId) -> EngineOutcome {
    if let Err(rejection) = validate_basic_action(cx.state, "Resource", investigator) {
        return rejection;
    }

    // Mutate-second: spend the action, fire AoO, then gain the resource.
    spend_one_action(cx, investigator);
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    // If AoO eliminated the investigator, the gain is suppressed; the
    // spent action + AoO events stay (mirrors `investigate`).
    let inv_after = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "Resource: investigator {investigator:?} disappeared between AoO and gain; \
             this is a state-corruption invariant violation"
            )
        });
    if inv_after.status != Status::Active {
        return EngineOutcome::Done;
    }

    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked above");
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
/// Mutate-second: spend 1 action, fire attacks of opportunity (Engage is
/// NOT `AoO`-exempt — the target is not engaged yet so it cannot `AoO`;
/// only OTHER engaged ready enemies do), then — if the investigator
/// survived —
/// engage the enemy.
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

    // Mutate-second.
    spend_one_action(cx, investigator);
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    let inv_after = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "Engage: investigator {investigator:?} disappeared between AoO and engagement; \
             this is a state-corruption invariant violation"
            )
        });
    if inv_after.status != Status::Active {
        return EngineOutcome::Done;
    }

    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "Engage: enemy {enemy_id:?} disappeared between validation and engagement; \
             this is a state-corruption invariant violation"
        )
    });
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
// Pre-existing bulk (99/100 lines before the Cx migration); the longer
// `cx.state.` qualifier nudged it past the limit without adding logic.
#[allow(clippy::too_many_lines)]
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

    // Move triggers attacks of opportunity from each ready engaged
    // enemy. Per the Rules Reference, this happens BEFORE the move
    // resolves.
    super::combat::fire_attacks_of_opportunity(cx, investigator);

    // If AoO defeated the investigator, the move is cancelled. The
    // action point and AoO events stay; the investigator (and any
    // engaged enemies) don't change location.
    let inv_after_aoo = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
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
        .expect("investigator existence checked above")
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
