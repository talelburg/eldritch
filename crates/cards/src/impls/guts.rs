//! Guts (Neutral skill, 01089).
//!
//! ```text
//! Max 1 committed per skill test.
//! 2 willpower icons.
//! If this test is successful, draw 1 card.
//! ```
//!
//! The 2 willpower icons and the "Max 1 committed per skill test" cap are
//! printed metadata (`SkillIcons` + `CardKind::Skill.commit_limit`,
//! enforced at the commit window, #311), not `abilities()`. `abilities()`
//! describes only the triggered effect: an `OnSkillTestResolution` gated on
//! `Success` that draws 1 card. No kind narrowing — it draws on **any**
//! successful test the card is committed to (unlike Deduction's
//! Investigate-only bonus).

use card_dsl::dsl::{
    draw_cards, on_skill_test_resolution, Ability, InvestigatorTarget, TestOutcome,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01089";

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
