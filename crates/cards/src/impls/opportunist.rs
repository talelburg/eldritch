//! Opportunist (Rogue skill, 01053).
//!
//! ```text
//! Commit only to a skill test you are performing.
//! If you succeed by 3 or more, return Opportunist to your hand after this
//!   test instead of discarding it.
//! 1 wild icon.
//! ```
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! The wild icon and *"Commit only to a skill test you are performing."* are
//! printed metadata (`SkillIcons` and `CardKind::Skill.commit_only_to_own_test`,
//! enforced at the commit window), not `abilities()`.
//!
//! The return is the card's own success effect: an `OnSkillTestResolution`
//! gated on `Success` that, when [`Quantity::SkillTestSucceededBy`] is 3 or
//! more, runs a native that sets the card's ST.8 destination to its hand. The
//! native names its own card through the committed-card binding the engine
//! gives every `OnSkillTestResolution` effect. A card-local native rather than
//! a DSL effect: Opportunist is the only implemented card returning a
//! committed card (Try and Try Again 02309 would be the second).
//!
//! [`Quantity::SkillTestSucceededBy`]: card_dsl::dsl::Quantity::SkillTestSucceededBy

use card_dsl::dsl::{self, Ability, TestOutcome};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01053";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(RETURN_TO_HAND, return_to_hand)]);

const RETURN_TO_HAND: &str = "01053:return-to-hand";

/// On a success by 3 or more, return this card to hand after the test.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_skill_test_resolution(
        TestOutcome::Success,
        dsl::if_(dsl::succeeded_by(3), dsl::native(RETURN_TO_HAND)),
    )]
}

/// Send this committed card to hand, not the discard pile, when the test ends.
fn return_to_hand(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(index) = ctx.committed_card() else {
        return EngineOutcome::Rejected {
            reason: "Opportunist 01053: no committed card bound; the return to hand runs only \
                     as the card's own skill-test result"
                .into(),
        };
    };
    cx.return_committed_card_to_hand(index);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    use super::*;

    #[test]
    fn ability_is_a_success_gated_return_to_hand() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnSkillTestResolution {
                outcome: TestOutcome::Success,
            },
        );
        let Effect::If {
            condition, then, ..
        } = &abilities[0].effect
        else {
            panic!("expected an If, got {:?}", abilities[0].effect);
        };
        assert_eq!(*condition, dsl::succeeded_by(3));
        assert!(matches!(&**then, Effect::Native { tag } if tag == RETURN_TO_HAND));
    }
}
