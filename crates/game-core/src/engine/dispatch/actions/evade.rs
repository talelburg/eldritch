//! The Evade basic action: an Agility test against an enemy engaged with
//! the investigator.

use card_dsl::dsl::{ActionClass, SkillTestKind};

use crate::engine::dispatch::skill_test;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{
    DifficultyBasis, Enemy, EnemyId, GameState, InvestigatorId, SkillKind, SkillTestFollowUp,
};

use super::{charge_action, validate_basic_action};

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

/// Handler for `TurnAction::Evade`.
///
/// Spends 1 action, runs an Agility skill test against the enemy's
/// evade value, and on success disengages and exhausts the enemy.
pub(in crate::engine::dispatch) fn evade(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
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
