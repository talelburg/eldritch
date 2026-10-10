//! The Evade basic action and its candidates: an Agility test against an
//! enemy engaged with the investigator.

use card_dsl::dsl::SkillTestKind;

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::skill_test;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{
    ActionResume, DifficultyBasis, Enemy, EnemyId, GameState, InvestigatorId, SkillKind,
    SkillTestFollowUp,
};

/// The enemies an Evade may target: every enemy engaged with `investigator`, in
/// ascending [`EnemyId`] order. `glossary/Evade_Action.md`: *"To evade an enemy
/// engaged with an investigator"*, and *"Unlike the fight and engage action, an
/// investigator can only perform an evade action against an enemy engaged with
/// him or her."* That is narrower than Fight's co-located
/// [`candidates`](super::fight::candidates).
///
/// The rules scope only; a malformed evade value is [`has_malformed_value`]'s
/// question. Read by the basic action's target validation and by the turn
/// menu. A designated **Evade** is not implemented yet (`TODO(#818)`), so the
/// designator gate rejects it without reading this.
pub(crate) fn candidates(state: &GameState, investigator: InvestigatorId) -> Vec<EnemyId> {
    state
        .enemies
        .iter()
        .filter(|(_, enemy)| enemy.engaged_with == Some(investigator))
        .map(|(&id, _)| id)
        .collect()
}

/// Whether `enemy`'s printed evade value is malformed (negative), which makes
/// it no Evade target even when it is a candidate.
pub(crate) fn has_malformed_value(enemy: &Enemy) -> bool {
    enemy.evade < 0
}

/// Handler for `TurnAction::Evade`.
///
/// Spends 1 action, runs an Agility skill test against the enemy's
/// evade value, and on success disengages and exhausts the enemy.
///
/// Validate-first: the investigator may take the action ([`take::check`]), the
/// enemy is in state and one of the [`candidates`], and its evade value is not
/// malformed. Then take it ([`take::take`]). Evade is
/// on the attack-of-opportunity exempt list, so taking it performs the evade at
/// once ([`perform_evade`]).
pub(in crate::engine::dispatch) fn evade(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Evade);
    if let Err(reason) = take::check(cx.state, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Rejected {
            reason: format!("Evade: enemy {enemy_id:?} is not in state").into(),
        };
    };
    if !candidates(cx.state, investigator).contains(&enemy_id) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Evade: {investigator:?} is not engaged with {enemy_id:?} (engaged_with = {:?})",
                enemy.engaged_with,
            )
            .into(),
        };
    }
    // The evade value it range-checks is read again at ST.6 through the
    // modified-value query, not carried out of here (#677). A malformed
    // `evade: -1` rejects with a clear reason rather than being clamped.
    if has_malformed_value(enemy) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Evade: enemy {enemy_id:?} has negative evade value {} (malformed state)",
                enemy.evade,
            )
            .into(),
        };
    }
    take::take(cx, investigator, &description, |_| {
        Ok(ActionResume::Evade { enemy: enemy_id })
    })
}

/// Perform an **evade** against `enemy_id`: an Agility test whose difficulty
/// *is* that enemy's modified evade value, read at ST.6, which on success
/// disengages and exhausts the enemy.
///
/// Callers validate the target; this takes the id as given.
pub(in crate::engine::dispatch) fn perform_evade(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
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
