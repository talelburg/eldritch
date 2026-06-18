//! Mind over Matter (Seeker insight event, 01036).
//!
//! ```text
//! Fast. Play only during your turn.
//! Until the end of the round, you may use your [intellect] in place of your
//!   [combat] and [agility].
//! ```
//!
//! One `OnPlay` native: push a round-scoped [`SkillSubstitution`] letting the
//! controller make a Combat/Agility test as an Intellect test instead. The
//! choice is offered at test initiation (intellect/wild icons + intellect
//! bonuses apply, a weapon's combat bonus is dropped) — see the engine's
//! substitution prompt. "Fast" + "Play only during your turn" come from the
//! corpus metadata (`Fast.` ⇒ `is_fast`; the clause ⇒
//! `CardMetadata::play_only_during_turn()`), enforced by the play-card gate —
//! no per-card play-timing code here.

use card_dsl::dsl::{native, on_play, Ability};
use game_core::card_data::SkillKind;
use game_core::card_registry::NativeEffectFn;
use game_core::state::SkillSubstitution;
use game_core::{Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01036";

const SUBSTITUTE: &str = "01036:intellect-substitution";

/// `OnPlay`: activate the round-scoped Intellect-for-Combat/Agility
/// substitution.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(native(SUBSTITUTE))]
}

/// Resolve this card's native-effect tag. Wired into the crate registry's
/// `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == SUBSTITUTE).then_some(substitute as NativeEffectFn)
}

/// Push the round-scoped substitution for the controller.
fn substitute(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    cx.state.skill_substitutions.push(SkillSubstitution {
        investigator: ctx.controller,
        use_skill: SkillKind::Intellect,
        for_skills: vec![SkillKind::Combat, SkillKind::Agility],
    });
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

    #[test]
    fn native_resolves_only_its_tag() {
        assert!(super::native_effect_for(super::SUBSTITUTE).is_some());
        assert!(super::native_effect_for("01036:other").is_none());
    }
}
