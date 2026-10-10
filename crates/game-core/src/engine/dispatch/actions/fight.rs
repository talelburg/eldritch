//! The Fight basic action, its target validation, and the fight primary
//! shared with the designated Fight.

use card_dsl::dsl::{IntExpr, SkillTestKind, Stat};

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::skill_test;
use crate::engine::dispatch::skill_test::InitiatorModifier;
use crate::engine::outcome::EngineOutcome;
use crate::engine::{designator, Cx};
use crate::state::{
    AbilitySource, ActionResume, DifficultyBasis, EnemyId, GameState, InvestigatorId,
    ModifierTarget, SkillKind, SkillTestFollowUp,
};

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

/// Handler for `TurnAction::Fight`.
///
/// Spends 1 action, runs a Combat skill test against the enemy's
/// fight value, and on success deals 1 damage. If damage reaches
/// `max_health`, the enemy is defeated and removed from play.
///
/// Per Rules Reference p.12 ("To fight an enemy **at his or her location**…"),
/// Fight targets any enemy at the investigator's location — engaged with them or
/// not (unlike Evade, which is engagement-only; RR p.11). The eligibility check
/// is co-location, mirroring [`engage`](super::engage::engage) (#401).
///
/// Validate-first: the investigator may take the action ([`take::check`]), then
/// the target checks. Then take it ([`take::take`]). Fight is on the
/// attack-of-opportunity exempt list, so taking it performs the fight at once.
pub(in crate::engine::dispatch) fn fight(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Fight);
    if let Err(reason) = take::check(cx.state, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let inv = &cx.state.investigators[&investigator];
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
    take::take(cx, investigator, &description, |_| {
        Ok(ActionResume::Fight { enemy: enemy_id })
    })
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
