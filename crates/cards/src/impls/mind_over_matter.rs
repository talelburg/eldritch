//! Mind over Matter (Seeker insight event, 01036).
//!
//! ```text
//! Fast. Play only during your turn.
//! Until the end of the round, you may use your [intellect] in place of your
//!   [combat] and [agility].
//! ```
//!
//! One `OnPlay` native: register a round-scoped [`SkillSubstitution`] letting the
//! controller make a Combat/Agility test as an Intellect test instead. The
//! choice is offered at test initiation (intellect/wild icons + intellect
//! bonuses apply, a weapon's combat bonus is dropped) — see the engine's
//! substitution prompt. "Fast" + "Play only during your turn" come from the
//! corpus metadata (`Fast.` ⇒ `is_fast`; the clause ⇒
//! `CardMetadata::play_only_during_turn()`), enforced by the play-card gate —
//! no per-card play-timing code here.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{self, Ability};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, Cx, EngineOutcome};
use game_core::state::SkillSubstitution;

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01036";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(SUBSTITUTE, substitute)]);

const SUBSTITUTE: &str = "01036:intellect-substitution";

/// `OnPlay`: activate the round-scoped Intellect-for-Combat/Agility
/// substitution.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::native(SUBSTITUTE))]
}

/// Register the round-scoped substitution for the controller.
fn substitute(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    engine::add_skill_substitution(
        cx,
        SkillSubstitution {
            investigator: ctx.controller,
            use_skill: SkillKind::Intellect,
            for_skills: vec![SkillKind::Combat, SkillKind::Agility],
        },
    );
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn ability_is_on_play_native() {
        let a = super::abilities();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].trigger, Trigger::OnPlay);
        assert!(matches!(&a[0].effect, Effect::Native { tag } if tag == super::SUBSTITUTE));
    }
}
