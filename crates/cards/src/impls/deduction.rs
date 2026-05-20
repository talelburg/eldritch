//! Deduction (Seeker skill, 01039).
//!
//! ```text
//! Practiced.
//! 1 intellect icon.
//! If this skill test is successful while investigating a location,
//! discover 1 additional clue at that location.
//! ```
//!
//! The 1 intellect icon is part of the card's printed metadata
//! (`SkillIcons { intellect: 1, .. }`), not part of `abilities()` —
//! the icon-contribution path lives in the skill-test commit window
//! (`finish_skill_test` reads icons via the registry's
//! `metadata_for`), not through the DSL.
//!
//! `abilities()` describes only the triggered effect: a
//! [`Trigger::OnSkillTestResolution`] gated on
//! [`TestOutcome::Success`], with a nested [`if_`] over
//! [`Condition::SkillTestKind`] of
//! [`SkillTestKind::Investigate`] so the bonus only fires for the
//! Investigate action's test (not for treachery-driven intellect
//! tests, etc.). The bonus discovers 1 clue at the tested
//! location — read off the in-flight test's snapshot via
//! [`LocationTarget::TestedLocation`], not the controller's current
//! location, so card-derived Investigate variants that test at a
//! different location still resolve correctly.
//!
//! [`Trigger::OnSkillTestResolution`]: card_dsl::dsl::Trigger::OnSkillTestResolution
//! [`TestOutcome::Success`]: card_dsl::dsl::TestOutcome::Success
//! [`if_`]: card_dsl::dsl::if_
//! [`Condition::SkillTestKind`]: card_dsl::dsl::Condition::SkillTestKind
//! [`SkillTestKind::Investigate`]: card_dsl::dsl::SkillTestKind::Investigate
//! [`LocationTarget::TestedLocation`]: card_dsl::dsl::LocationTarget::TestedLocation

use card_dsl::dsl::{
    discover_clue, if_, on_skill_test_resolution, Ability, Condition, LocationTarget,
    SkillTestKind, TestOutcome,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01039";

/// On successful Investigate, discover 1 additional clue at the
/// tested location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_skill_test_resolution(
        TestOutcome::Success,
        if_(
            Condition::SkillTestKind(SkillTestKind::Investigate),
            discover_clue(LocationTarget::TestedLocation, 1),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Condition, Effect, LocationTarget, SkillTestKind, TestOutcome, Trigger};

    #[test]
    fn abilities_are_one_resolution_trigger_with_kind_narrowing() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnSkillTestResolution {
                outcome: TestOutcome::Success,
            },
        );
        // The effect is If(SkillTestKind(Investigate), DiscoverClue@TestedLocation).
        let Effect::If {
            condition, then, ..
        } = &abilities[0].effect
        else {
            panic!("expected Effect::If, got {:?}", abilities[0].effect);
        };
        assert_eq!(
            condition,
            &Condition::SkillTestKind(SkillTestKind::Investigate),
        );
        assert!(matches!(
            **then,
            Effect::DiscoverClue {
                from: LocationTarget::TestedLocation,
                count: 1,
            },
        ));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
