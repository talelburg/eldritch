//! Blinding Light (Mystic event, 01066).
//!
//! ```text
//! Evade. This evasion attempt uses [willpower] instead of [agility]. If you
//! succeed, deal 1 damage to the enemy just evaded. If a [skull], [cultist],
//! [tablet], [elder_thing], or [auto_fail] symbol is revealed during this
//! evasion attempt, lose 1 action this turn.
//! ```
//!
//! **Designator: Evade**, on the event's `OnPlay` ability. Playing it **performs
//! an evade action** against an enemy engaged with you, modified as the card
//! prints: it tests `[willpower]` in place of `[agility]` (the designator's
//! skill override), carries the lost action onto the test as a
//! [`RevealClause`], and deals the damage as the test's success clause. The
//! ability's own effect is empty. Being an evade action, the play provokes no
//! attack of opportunity, and with no enemy engaged it cannot be played.
//!
//! The lost action is not a reaction to the token. Ruling
//! (<https://arkhamdb.com/card/01066>): *"The "If a symbol is revealed..."
//! effect triggers during Step 3 of the Skill Test Timing."* So it resolves
//! inside the test, as the token is revealed, whether the evasion then
//! succeeds or fails.
//!
//! The damage resolves inside the test too, at step 7, once the enemy is
//! evaded: the native reads the evaded enemy off the in-flight test's
//! follow-up, the one place *"the enemy just evaded"* is named.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{self, Ability, ChaosSymbol, RevealClause};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};
use game_core::state::SkillTestFollowUp;

use crate::impls::CardRecord;

/// `ArkhamDB` code for Blinding Light.
pub const CODE: &str = "01066";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[(DAMAGE_EVADED, damage_evaded), (LOSE_ACTION, lose_action)]);

/// *"If you succeed, deal 1 damage to the enemy just evaded."*
const DAMAGE_EVADED: &str = "01066:damage-evaded";
/// *"… lose 1 action this turn."*
const LOSE_ACTION: &str = "01066:lose-action";

/// On play, an Evade testing `[willpower]` in place of `[agility]`: 1 damage
/// to the evaded enemy on success, and 1 action lost if a non-elder-sign symbol
/// is revealed.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play_as(
        dsl::evade()
            .testing(SkillKind::Willpower)
            .with_reveal_clause(RevealClause {
                symbols: ChaosSymbol::ALL_BUT_ELDER_SIGN.to_vec(),
                effect: Box::new(dsl::native(LOSE_ACTION)),
            })
            .on_success(dsl::native(DAMAGE_EVADED)),
        dsl::seq([]),
    )]
}

/// *"If you succeed, deal 1 damage to the enemy just evaded."* Runs as the
/// evade test's success clause, so the test is still in flight and its
/// follow-up names the enemy it evaded. Any other context is an engine fault,
/// rejected loudly rather than guessed at.
fn damage_evaded(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let enemy = match cx.state().current_skill_test().map(|t| &t.follow_up) {
        Some(SkillTestFollowUp::Evade { enemy }) => *enemy,
        other => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "{DAMAGE_EVADED} runs only as an evade test's success clause \
                     (follow-up was {other:?})"
                )
                .into(),
            }
        }
    };
    // The enemy may have left play while the test resolved; then there is no
    // enemy just evaded to damage.
    if cx.state().enemies.contains_key(&enemy) {
        cx.deal_damage_to_enemy(enemy, 1, Some(ctx.controller));
    }
    EngineOutcome::Done
}

/// *"… lose 1 action this turn."* The reveal clause's effect, so it runs with
/// the evading investigator as its controller.
fn lose_action(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.lose_actions(ctx.controller, 1);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Trigger};

    use super::*;

    #[test]
    fn it_is_one_on_play_ability_designating_a_willpower_evade() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert!(matches!(
            abilities[0].designator,
            Some(ActionDesignator::Evade {
                skill_override: Some(SkillKind::Willpower),
                reveal_clause: Some(_),
                on_success: Some(_),
            })
        ));
        assert_eq!(abilities[0].effect, dsl::seq([]));
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
