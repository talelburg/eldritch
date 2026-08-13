//! Machete (Guardian melee weapon asset, 01020).
//!
//! ```text
//! [action]: Fight. You get +1 [combat] for this attack. If the attacked
//! enemy is the only enemy engaged with you, this attack deals +1 damage.
//! ```
//!
//! A bare `[action]` Fight (no exhaust, no uses) with a flat `+1` combat
//! modifier. The bonus damage is conditional on the **attacked** enemy, not on
//! the controller's engaged count alone: `Effect::Fight`'s candidate scope is
//! every enemy *at your location* (#451), so the target may be one you are not
//! engaged with at all. `sole_engaged_target` therefore asks whether the set of
//! enemies engaged with you is exactly `{the chosen target}`.
//!
//! `ArkhamDB` FAQ (01020, last updated 5/4/17):
//!
//! ```text
//! Machete will not provide a damage bonus for attacking a disengaged enemy
//! (evaded or Aloof), or an enemy engaged to another player. You can spend an
//! action to Engage an enemy to gain the damage bonus.
//! ```
//!
//! Encoding the conjunction needs a predicate over the *chosen* target, which
//! the declarative vocabulary cannot express (no `Condition` combinator, no
//! target-referencing `Condition`/`Quantity`), so it rides `Condition::Native`
//! (#592). `TODO(#609)`: promote to declarative vocab when a second card wants
//! either half.
//!
//! All three branches are reachable: with exactly one co-located enemy the
//! target auto-binds (#449); with two or more the player picks which to attack;
//! and the picked enemy may be engaged with you, with another investigator, or
//! with nobody.

use card_dsl::dsl::{activated, fight, native_condition, Ability, IntExpr};
use game_core::card_registry::NativeConditionFn;
use game_core::state::GameState;
use game_core::EvalContext;

/// `ArkhamDB` code for Machete (original-Core printing).
pub const CODE: &str = "01020";

/// Native condition tag: the attacked enemy is the only enemy engaged with you.
const SOLE_ENGAGED_TAG: &str = "01020:sole_engaged_target";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![],
        fight(1u8, IntExpr::cond(native_condition(SOLE_ENGAGED_TAG), 1, 0)),
    )]
}

/// "If the attacked enemy is the only enemy engaged with you" — true iff the
/// enemies engaged with the controller are exactly the one being attacked.
///
/// Reads the Fight target from [`EvalContext::chosen_enemy`], which
/// `ground_chosen_targets` binds before `apply_fight` evaluates `extra_damage`.
/// An unbound target (no Fight in flight) is `false`: no attacked enemy, no
/// bonus. Both FAQ exclusions fall out of the set equality — an enemy engaged
/// with nobody or with another investigator is not in the controller's engaged
/// set, so the set can never equal `{target}` while it holds that target.
///
/// Engagement is read through [`GameState::enemies_engaged_with`] rather than
/// re-filtered here, so this predicate and the kernel's
/// [`Quantity::EngagedEnemies`](card_dsl::dsl::Quantity::EngagedEnemies) can't
/// drift to two readings of "engaged with you" — which is the shape of the bug
/// this card had (#592).
///
/// `TODO(#579)`: the card's second FAQ ruling is **not** honoured — "Machete
/// will provide a damage bonus for attacking a Massive enemy, as long as it is
/// ready and the only enemy engaged with you. A Massive enemy is 'considered'
/// engaged with you". `Massive` is unparsed corpus-wide, so such an enemy is
/// absent from the engaged set and the bonus is withheld. Under-granting, not
/// over-granting; #579 tracks the keyword, and this is its first live consumer.
fn sole_engaged_target(state: &GameState, ctx: &EvalContext) -> bool {
    let Some(target) = ctx.chosen_enemy() else {
        return false;
    };
    let mut engaged = state.enemies_engaged_with(ctx.controller).map(|(id, _)| id);
    engaged.next() == Some(target) && engaged.next().is_none()
}

/// Resolve Machete's native condition tag.
pub(crate) fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    match tag {
        SOLE_ENGAGED_TAG => Some(sole_engaged_target as NativeConditionFn),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_costless_activated_fight_ability() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(
            abilities[0].costs.is_empty(),
            "Machete's Fight has no exhaust/uses cost — just the action",
        );
        let Effect::Fight {
            combat_modifier,
            extra_damage,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::Fight");
        };
        assert_eq!(*combat_modifier, IntExpr::Lit(1));
        // +1 only when the *attacked* enemy is the sole engaged enemy (#592).
        assert_eq!(
            *extra_damage,
            IntExpr::cond(native_condition(SOLE_ENGAGED_TAG), 1, 0)
        );
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }

    #[test]
    fn native_condition_tag_resolves() {
        assert!(native_condition_for(SOLE_ENGAGED_TAG).is_some());
        assert!(native_condition_for("nope").is_none());
        assert!(
            crate::impls::native_condition_for(SOLE_ENGAGED_TAG).is_some(),
            "the crate-level dispatch must route Machete's tag here",
        );
    }

    /// The predicate itself, across the four target/engagement shapes the
    /// widened Fight scope (#451) can produce. `EnemyId(1)` is the attacked
    /// enemy throughout; `EnemyId(2)` is a bystander.
    #[test]
    fn sole_engaged_target_is_set_equality_not_a_count() {
        use game_core::state::{EnemyId, GameStateBuilder, InvestigatorId};

        const ACTOR: InvestigatorId = InvestigatorId(1);
        const OTHER: InvestigatorId = InvestigatorId(2);
        const TARGET: EnemyId = EnemyId(1);

        /// Board where each enemy is engaged with `engaged_with[i]`.
        fn board(engagements: &[(u32, Option<InvestigatorId>)]) -> GameState {
            let mut builder = GameStateBuilder::new()
                .with_investigator(game_core::test_support::test_investigator(1))
                .with_investigator(game_core::test_support::test_investigator(2));
            for (id, engaged_with) in engagements {
                let mut enemy = game_core::test_support::test_enemy(*id, "Ghoul");
                enemy.engaged_with = *engaged_with;
                builder = builder.with_enemy(enemy);
            }
            builder.build()
        }

        let mut ctx = EvalContext::for_controller(ACTOR);
        ctx.set_chosen_enemy(TARGET);

        assert!(
            sole_engaged_target(&board(&[(1, Some(ACTOR))]), &ctx),
            "target is the only enemy engaged with you → bonus"
        );
        assert!(
            !sole_engaged_target(&board(&[(1, Some(ACTOR)), (2, Some(ACTOR))]), &ctx),
            "a second enemy engaged with you → no bonus"
        );
        assert!(
            !sole_engaged_target(&board(&[(1, None), (2, Some(ACTOR))]), &ctx),
            "attacking an unengaged enemy while engaged with exactly one other → no bonus",
        );
        assert!(
            !sole_engaged_target(&board(&[(1, None)]), &ctx),
            "engaged with nothing at all → no bonus",
        );
        assert!(
            !sole_engaged_target(&board(&[(1, Some(OTHER))]), &ctx),
            "enemy engaged to another player → no bonus (FAQ)",
        );
        // The two shapes above agree with the old count-only encoding (the
        // actor is engaged with nothing, so it read 0 too). This one does not:
        // the actor is engaged with exactly one enemy, so `EngagedEnemies == 1`
        // held and the bonus was wrongly granted before #592.
        assert!(
            !sole_engaged_target(&board(&[(1, Some(OTHER)), (2, Some(ACTOR))]), &ctx),
            "target engaged to another player while you are engaged with exactly \
             one *other* enemy → no bonus",
        );

        let unbound = EvalContext::for_controller(ACTOR);
        assert!(
            !sole_engaged_target(&board(&[(1, Some(ACTOR))]), &unbound),
            "no attacked enemy bound → no bonus",
        );
    }
}
