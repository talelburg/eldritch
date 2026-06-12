//! Player-action handlers: Investigate, Move, Fight, Evade, plus the
//! engaged-action validation and single-action-spend helpers.

use crate::dsl::SkillTestKind;
use crate::event::Event;
use crate::state::{
    Enemy, EnemyId, GameState, InvestigatorId, LocationId, Phase, SkillKind, SkillTestFollowUp,
    Status,
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
    // Validate-first.
    if cx.state.phase != Phase::Investigation {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate is only valid during the Investigation phase (was {:?})",
                cx.state.phase
            )
            .into(),
        };
    }
    if cx.state.active_investigator != Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: {investigator:?} is not the active investigator ({:?})",
                cx.state.active_investigator,
            )
            .into(),
        };
    }
    // Active-investigator + missing-from-map is a state-corruption
    // invariant violation; panic rather than silently rejecting.
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "Investigate: active_investigator {investigator:?} is not in the investigators \
             map; this is a state-corruption invariant violation"
            )
        });
    if inv.status != Status::Active {
        return EngineOutcome::Rejected {
            reason: format!(
                "Investigate: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        };
    }
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Investigate requires at least 1 action point".into(),
        };
    }
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
    // at i8::MAX for the absurd case; realistic shrouds are 0–6.
    let difficulty = i8::try_from(location.shroud).unwrap_or(i8::MAX);

    // Mutate-second: spend the action, fire AoO, then resolve the
    // test. Investigate is NOT on the AoO-exempt list (only Fight,
    // Evade, Parley, Engage, Resign are), so each ready engaged
    // enemy attacks before the test resolves.
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
    )
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
    if inv.actions_remaining < 1 {
        return EngineOutcome::Rejected {
            reason: "Move requires at least 1 action point".into(),
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

    // Mutate-second.
    spend_one_action(cx, investigator);

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
    // Terminal step: the entered location's Forced on-enter abilities fire,
    // and their outcome becomes the move's outcome. This runs *after* the
    // move is applied, so if it returns Rejected (e.g. 2+ simultaneous
    // forced triggers, #213), `apply`'s structural rollback restores the
    // pre-move state — the partial mutation above is safe (same reliance on
    // the apply-loop snapshot that `play_card` documents).
    super::forced_triggers::fire_forced_triggers(
        cx,
        &super::forced_triggers::ForcedTriggerPoint::EnteredLocation {
            investigator,
            location: destination,
        },
    )
}

/// Validate the prefix shared by Fight and Evade: phase, active
/// investigator, action point available, enemy exists, engaged with
/// the named enemy. Returns the borrowed enemy so the caller can pick
/// which difficulty (fight / evade) and read any other fields it
/// needs.
///
/// On `Err`, returns the rejection; the caller should propagate it
/// without further state mutation. State-corruption invariants
/// (active investigator missing from map) panic via `unreachable!`.
///
/// Does NOT validate the chosen difficulty is non-negative — the
/// caller must do that after picking, because Fight and Evade each
/// only care about one of the two values, and validating both
/// upfront would reject legitimate states (an enemy with `fight: -1`
/// the investigator only ever Evades).
fn validate_engaged_action<'a>(
    state: &'a GameState,
    action_name: &'static str,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> Result<&'a Enemy, EngineOutcome> {
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
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked before spend_one_action");
    let new_count = inv.actions_remaining - 1;
    inv.actions_remaining = new_count;
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

/// Handler for [`PlayerAction::Fight`].
///
/// Spends 1 action, runs a Combat skill test against the enemy's
/// fight value, and on success deals 1 damage. If damage reaches
/// `max_health`, the enemy is defeated and removed from play.
///
/// Damage > 1 (weapons, card buffs), after-success / after-failure
/// triggers (#64), and `AoO` from *other* engaged enemies (#78) are all
/// downstream. `AoO` does NOT fire on Fight itself per the Rules
/// Reference's `AoO`-exempt list.
pub(super) fn fight(cx: &mut Cx, investigator: InvestigatorId, enemy_id: EnemyId) -> EngineOutcome {
    let fight_difficulty = match validate_engaged_action(cx.state, "Fight", investigator, enemy_id)
    {
        Ok(enemy) => {
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
        }
        Err(rejected) => return rejected,
    };
    spend_one_action(cx, investigator);
    super::skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Combat,
        SkillTestKind::Fight,
        fight_difficulty,
        SkillTestFollowUp::Fight { enemy: enemy_id },
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
    spend_one_action(cx, investigator);
    super::skill_test::start_skill_test(
        cx,
        investigator,
        SkillKind::Agility,
        SkillTestKind::Evade,
        evade_difficulty,
        SkillTestFollowUp::Evade { enemy: enemy_id },
    )
}
