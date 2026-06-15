//! Manual Dexterity (Neutral skill, 01092).
//!
//! ```text
//! Max 1 committed per skill test.
//! 2 agility icons.
//! If this test is successful, draw 1 card.
//! ```
//!
//! Same shape as Guts 01089 (agility icons). Icons + the "Max 1 committed"
//! cap are printed metadata (#311); `abilities()` is the
//! `OnSkillTestResolution { Success }` draw, firing on any successful
//! committed-to test.

use card_dsl::dsl::{
    draw_cards, on_skill_test_resolution, Ability, InvestigatorTarget, TestOutcome,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01092";

/// On any successful test this is committed to, draw 1 card.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_skill_test_resolution(
        TestOutcome::Success,
        draw_cards(InvestigatorTarget::You, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, InvestigatorTarget, TestOutcome, Trigger};

    #[test]
    fn abilities_are_one_on_success_draw_one() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnSkillTestResolution {
                outcome: TestOutcome::Success,
            },
        );
        assert_eq!(
            abilities[0].effect,
            Effect::DrawCards {
                target: InvestigatorTarget::You,
                count: 1,
            },
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
