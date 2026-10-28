//! The Fight basic action, its candidates and target validation, the fight
//! primary shared with the designated Fight, its after-test damage, and the
//! retaliate attack a failed fight provokes.

use card_dsl::dsl::{IntExpr, RevealClause};

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::skill_test::SkillTestSpec;
use crate::engine::dispatch::{combat, skill_test};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::state::{
    AbilitySource, ActionResume, Enemy, EnemyId, GameState, InvestigatorId, SkillKind,
    SkillTestFollowUp,
};

/// The enemies a Fight may target: every enemy at `investigator`'s location, in
/// ascending [`EnemyId`] order. `glossary/Fight_Action.md`: *"An investigator
/// may fight any enemy at his or her location, including: an enemy he or she is
/// engaged with, an unengaged enemy at the same location, or an enemy engaged
/// with another investigator who is at the same location."*
///
/// The rules scope only. A malformed fight value is a separate question,
/// [`has_malformed_value`], which the basic action and the turn menu ask on top;
/// a designated **Fight**'s grounding does not.
///
/// Every caller that needs the Fight targets reads this: the basic action's
/// target validation, the turn menu, the designator's pre-cost gate
/// (`designator::can_perform`), and the evaluator's grounding of a designated
/// **Fight**'s pick. The basic action names its target up front, so it asks
/// whether *this* enemy is in the list; the gate asks only whether the list is
/// empty, and the evaluator offers the list as the pick.
pub(crate) fn candidates(state: &GameState, investigator: InvestigatorId) -> Vec<EnemyId> {
    combat::enemies_in_scope_before_exemption(state, investigator, &combat::fight_target_scope())
}

/// Whether `enemy`'s printed fight value is malformed (negative), which makes
/// it no Fight target for the basic action even when it is a candidate.
pub(crate) fn has_malformed_value(enemy: &Enemy) -> bool {
    enemy.fight < 0
}

/// Validate that `enemy_id` is a legal Fight target for `investigator`: it is
/// one of the [`candidates`], and its printed fight value is not malformed.
///
/// Returns nothing on success: the fight value it range-checks is read
/// again at ST.6 through the modified-value query, not carried out of
/// here.
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
    if !candidates(state, investigator).contains(&enemy_id) {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "Fight: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location",
                enemy.current_location,
            )
            .into(),
        });
    }
    if has_malformed_value(enemy) {
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
/// is co-location, mirroring [`engage`](super::engage::handle).
///
/// Validate-first: the investigator may take the action ([`take::check`]), then
/// the target checks. Then take it ([`take::take`]). Fight is on the
/// attack-of-opportunity exempt list, so taking it performs the fight at once.
pub(in crate::engine::dispatch) fn handle(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Fight);
    if let Err(reason) = take::check(cx.state, cx.cards, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let inv = &cx.state.investigators[&investigator];
    // A `None` location can't host a fight (mirrors `engage`); `candidates`
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

/// How an attack differs from a basic Fight: the modification a **Fight**
/// designator carries (`glossary/Ability.md`: *"performs the designated action
/// as described in the rules, but modified in the manner described by the
/// ability"*). [`Default`] is the basic Fight action's: a combat test, no
/// modifier, no extra damage, no clause.
#[derive(Debug, Clone, Default)]
pub(crate) struct FightModification {
    /// *"+N \[combat\] for this attack"*, applied to whichever skill the
    /// attack tests.
    pub combat_modifier: Option<IntExpr>,
    /// Damage beyond the base 1 (.38 Special: +1), unevaluated until
    /// [`after_test`] determines the attack's damage at RR ST.7. `None` is the
    /// basic Fight's, which deals no extra damage.
    pub extra_damage: Option<IntExpr>,
    /// The skill tested instead of `[combat]` (Shrivelling 01060's
    /// `[willpower]`).
    pub skill_override: Option<SkillKind>,
    /// The card's *"If a \<symbol\> is revealed during this attack"*.
    pub reveal_clause: Option<RevealClause>,
}

/// Perform a **fight** against `enemy_id`: a Combat test (or the skill
/// `modification` tests instead) whose difficulty *is* that enemy's modified
/// fight value, read at ST.6 rather than snapshotted here, dealing
/// `1 + extra_damage` on success.
///
/// The one primary behind both ways of attacking — the basic Fight
/// action and an ability printing the bold **Fight** designator (every weapon
/// in the corpus, and Shrivelling 01060). `glossary/Ability.md`: *"Activating
/// such an ability **performs the designated action** as described in the
/// rules, but modified in the manner described by the ability."* The
/// [`FightModification`] is the *only* thing that differs.
///
/// `combat_modifier` is *"+N \[combat\] for this attack"* — a row over the
/// controller's tested skill scoped to the test about to start, not a number
/// added to a snapshotted total. It travels unevaluated so the row answers from
/// the board at every read (ADR 0005), which is what makes Esoteric Formula
/// 02254's *"+2 \[willpower\] for this attack for each clue on the attacked
/// enemy"* expressible without a second mechanism. `extra_damage` travels
/// unevaluated too, on the Fight follow-up, to [`after_test`].
///
/// Callers validate the target; this takes the id as given.
pub(crate) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    modification: FightModification,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    skill_test::start_skill_test(
        cx,
        investigator,
        SkillTestSpec::Fight {
            enemy: enemy_id,
            modification,
        },
        source,
    )
}

/// A fight's after-test step (RR ST.7), run on success only: deal
/// `1 + extra_damage + bonus_attack_damage` to `enemy_id`.
///
/// `extra_damage` is evaluated here, when the attack's damage is determined,
/// not when the attack began. Its context is the ability's (`source`), with the
/// attacked enemy bound as the chosen enemy (Machete 01020's *"If the attacked
/// enemy is the only enemy engaged with you"*) and the test's success margin
/// bound (Switchblade 01044's *"If you succeed by 2 or more"*), so a card
/// committed after initiation counts towards it. A negative value deals no
/// less than the base. An expression the evaluator can't answer (an
/// unregistered native tag) is returned as the rejection the caller
/// propagates, so the apply rolls back as it did when the same error surfaced
/// at initiation.
///
/// The attacked enemy is still here. An enemy that left play *before* ST.6
/// abandons the test outright (the skill-test driver's preamble), which is
/// what keeps `damage_enemy`'s enemy-missing panic loud rather than reachable.
/// The residue it does not cover — an enemy removed by something firing in the
/// post-ST.6 `SkillTestResolved` run, before this step — has no corpus card that
/// can do it: the fast hand events are Dodge, Working a Hunch, Evidence and Mind
/// over Matter, none of which removes an enemy.
///
/// A `None` `extra_damage` (the basic Fight) contributes nothing.
///
/// A weapon's bonus damage (.38 Special's +1) rides on `extra_damage`; a
/// committed skill's bonus (Vicious Blow's +1) accumulates on the in-flight
/// record at commit time. The in-flight test is still present here —
/// it's torn down only at the end of resolution — so the accumulator, the
/// ability's source and the verdict are all readable.
pub(in crate::engine::dispatch) fn after_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    extra_damage: Option<&IntExpr>,
) -> Result<(), EngineOutcome> {
    let (bonus, source, resolved) = cx.state.current_skill_test().map_or((0, None, None), |t| {
        (t.bonus_attack_damage, t.source, t.resolved)
    });
    let mut ctx = EvalContext::for_controller_with_optional_source(investigator, source);
    ctx.set_chosen_enemy(enemy_id);
    if let Some(resolved) = resolved {
        ctx.bind_resolved(&resolved);
    }
    let extra = match extra_damage {
        Some(expr) => {
            evaluator::eval_int_expr(cx.state, cx.cards, &ctx, expr).map_err(|reason| {
                EngineOutcome::Rejected {
                    reason: reason.into(),
                }
            })?
        }
        None => 0,
    };
    combat::damage_enemy(
        cx,
        enemy_id,
        1u8.saturating_add(u8::try_from(extra).unwrap_or(0))
            .saturating_add(bonus),
        Some(investigator),
    );
    Ok(())
}

/// Fire a Retaliate attack if the just-resolved test was a *failed Fight*
/// against a ready enemy with the retaliate keyword. Only an attack triggers
/// retaliate, which is why it lives with Fight.
///
/// `glossary/Retaliate.md`: *"Each time an investigator fails a skill test
/// while attacking a ready enemy with the retaliate keyword, after applying all
/// results for that skill test, that enemy performs an attack against the
/// attacking investigator. An enemy does not exhaust after performing a
/// retaliate attack."*
///
/// The skill-test driver calls this at its `PostRetaliate` step — after
/// `fire_on_skill_test_resolution` (the rest of ST.7) and before the
/// `PostOnResolution` teardown (ST.8) — matching "after applying all results."
/// Routes through [`combat::drive_retaliate`] so the attack opens its
/// before-attack cancel window (Dodge 01023) and per-soaked-asset soak window
/// (Guard Dog 01021). Returns [`AwaitingInput`] if a window suspends,
/// [`Done`] otherwise. Non-exhausting — honored by
/// [`EnemyAttackSource::Retaliate`] inside `drive_retaliate`.
///
/// No-op unless every condition holds: the test failed; its follow-up was
/// `Fight`; the enemy is still in play, ready (`!exhausted`), and has
/// `retaliate`. A missing enemy is skipped quietly — a failed fight deals
/// no damage, so the target can't have been defeated mid-test; this only
/// guards against future enemy-removing commit effects.
///
/// [`AwaitingInput`]: crate::engine::EngineOutcome::AwaitingInput
/// [`Done`]: crate::engine::EngineOutcome::Done
/// [`EnemyAttackSource::Retaliate`]: crate::state::EnemyAttackSource::Retaliate
pub(in crate::engine::dispatch) fn fire_retaliate_if_any(
    cx: &mut Cx,
    investigator: InvestigatorId,
    succeeded: bool,
) -> EngineOutcome {
    if succeeded {
        return EngineOutcome::Done;
    }
    let Some(SkillTestFollowUp::Fight { enemy, .. }) =
        cx.state.current_skill_test().map(|t| &t.follow_up)
    else {
        return EngineOutcome::Done;
    };
    let enemy = *enemy;
    // A covered enemy (On the Lam 01010) may not attack, so it does not
    // retaliate either.
    let retaliates = cx.state.enemies.get(&enemy).is_some_and(|e| {
        e.retaliate && !e.exhausted && combat::enemy_may_attack(cx.state, cx.cards, e, investigator)
    });
    if retaliates {
        // Route through the attack loop so the retaliate opens its cancel
        // (Dodge) and soak (Guard Dog) windows; non-exhausting.
        combat::drive_retaliate(cx, enemy, investigator)
    } else {
        EngineOutcome::Done
    }
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::SkillTestKind;

    use super::*;
    use crate::state::{Continuation, GameStateBuilder, InFlightSkillTest, SkillTestId};
    use crate::test_support::{self, TEST_REGISTRIES};

    /// The `Fight` follow-up deals `1 + extra_damage + bonus_attack_damage`,
    /// reading the commit-time accumulator off the in-flight record
    /// (Vicious Blow 01025). With `extra_damage: 1` (a weapon bonus) and
    /// `bonus_attack_damage: 2`, the attack deals `1 + 1 + 2 = 4`.
    #[test]
    fn fight_follow_up_adds_bonus_attack_damage() {
        let inv = InvestigatorId(1);
        let mut enemy = test_support::test_enemy(7, "Goon");
        enemy.max_health = 10; // avoid clamping so the dealt damage is observable
        let mut state = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_seat(test_support::seat(1))
            .with_enemy(enemy)
            .build();
        state
            .continuations
            .push(Continuation::SkillTest(InFlightSkillTest {
                follow_up: SkillTestFollowUp::Fight {
                    enemy: EnemyId(7),
                    extra_damage: Some(IntExpr::Lit(1)),
                },
                bonus_attack_damage: 2,
                ..test_support::test_skill_test(
                    SkillTestId(0),
                    inv,
                    SkillKind::Combat,
                    SkillTestKind::Fight,
                    2,
                )
            }));
        let mut events = Vec::new();
        let mut cx = Cx::for_test(&mut state, &mut events);

        after_test(&mut cx, inv, EnemyId(7), Some(&IntExpr::Lit(1))).expect("a literal evaluates");

        assert_eq!(
            state.enemies[&EnemyId(7)].damage,
            4,
            "1 base + 1 extra_damage + 2 bonus_attack_damage"
        );
    }
}
