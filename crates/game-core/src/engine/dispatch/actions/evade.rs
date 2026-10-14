//! The Evade basic action, its candidates, the evade perform entry and its
//! after-test step: an Agility test against an enemy engaged with the
//! investigator, which on success disengages and exhausts it.

use card_dsl::dsl::{Effect, RevealClause, SkillTestKind};

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::dispatch::skill_test;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{
    AbilitySource, ActionResume, DifficultyBasis, Enemy, EnemyId, GameState, InvestigatorId,
    SkillKind, SkillTestFollowUp,
};

/// The enemies an Evade may target: every enemy engaged with `investigator`, in
/// ascending [`EnemyId`] order. `glossary/Evade_Action.md`: *"To evade an enemy
/// engaged with an investigator"*, and *"Unlike the fight and engage action, an
/// investigator can only perform an evade action against an enemy engaged with
/// him or her."* That is narrower than Fight's co-located
/// [`candidates`](super::fight::candidates).
///
/// The rules scope only; a malformed evade value is [`has_malformed_value`]'s
/// question. Read by the basic action's target validation, the turn menu, the
/// designator gate (`designator::can_perform`) and the evaluator's grounding of
/// a designated **Evade**'s target.
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
/// once ([`perform`]).
pub(in crate::engine::dispatch) fn handle(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Evade);
    if let Err(reason) = take::check(cx.state, cx.cards, investigator, &description) {
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

/// How an evasion attempt differs from a basic Evade: the modification an
/// **Evade** designator carries (`glossary/Ability.md`: *"performs the
/// designated action as described in the rules, but modified in the manner
/// described by the ability"*). [`Default`] is the basic Evade action's: an
/// agility test with no clause. The twin of
/// [`FightModification`](super::fight::FightModification).
#[derive(Debug, Clone, Default)]
pub(crate) struct EvadeModification {
    /// The skill tested instead of `[agility]` (Blinding Light 01066's
    /// `[willpower]`).
    pub skill_override: Option<SkillKind>,
    /// The card's *"If a \<symbol\> is revealed during this evasion attempt"*.
    pub reveal_clause: Option<RevealClause>,
    /// The card's *"If you succeed, …"*, resolved at ST.7 after the enemy is
    /// evaded, while the test (and so the evaded enemy on its follow-up) is
    /// still in flight.
    pub on_success: Option<Effect>,
}

/// Perform an **evade** against `enemy_id`: an Agility test (or the skill
/// `modification` tests instead) whose difficulty *is* that enemy's modified
/// evade value, read at ST.6, which on success disengages and exhausts the
/// enemy ([`after_test`]).
///
/// The one perform entry for both ways of evading, as
/// [`fight::perform`](super::fight::perform) is for fighting: the basic
/// Evade action reaches it after taking the action, and a designated **Evade**
/// (Blinding Light 01066) reaches it with its [`EvadeModification`], the only
/// thing that differs.
///
/// Callers validate the target; this takes the id as given.
pub(crate) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
    modification: EvadeModification,
    source: Option<AbilitySource>,
) -> EngineOutcome {
    let EvadeModification {
        skill_override,
        reveal_clause,
        on_success,
    } = modification;
    skill_test::start_skill_test(
        cx,
        investigator,
        skill_override.unwrap_or(SkillKind::Agility),
        SkillTestKind::Evade,
        // The difficulty *is* the enemy's modified evade value, read at
        // ST.6 — Cold Spring Glen 02244's "Each enemy in Cold Spring Glen
        // gets -1 evade" reaches this test (#677).
        DifficultyBasis::Evade(enemy_id),
        SkillTestFollowUp::Evade { enemy: enemy_id },
        on_success,
        None,
        source,
        None, // no Evade designator prints a skill modifier yet
        reveal_clause,
    )
}

/// An evade's after-test step (RR ST.7), run on success only: disengage
/// `enemy_id` from `investigator` and exhaust it. `glossary/Evade_Action.md`:
/// *"Any time an enemy is evaded (whether by an evade action, or by card
/// ability), the enemy is exhausted (if it was ready) and the engagement is
/// broken."*
///
/// The enemy is still in play: one that left play before ST.6 abandons the
/// test, so a missing enemy here is a state-corruption invariant violation.
pub(in crate::engine::dispatch) fn after_test(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) {
    let enemy = cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "Evade follow-up: enemy {enemy_id:?} vanished while test was in flight; \
             this is a state-corruption invariant violation"
        )
    });
    enemy.engaged_with = None;
    enemy.exhausted = true;
    cx.events.push(Event::EnemyDisengaged {
        enemy: enemy_id,
        investigator,
    });
    cx.events.push(Event::EnemyExhausted { enemy: enemy_id });
}
