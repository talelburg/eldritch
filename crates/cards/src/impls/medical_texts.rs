//! Medical Texts (Seeker item asset, 01035).
//!
//! ```text
//! [action] Choose an investigator at your location and test [intellect] (2).
//!   If you succeed, heal 1 damage from that investigator. If you fail, deal
//!   1 damage to that investigator.
//! ```
//!
//! One `[action]` ability (no exhaust, no uses): an
//! [`Effect::SkillTest`](card_dsl::dsl::Effect::SkillTest) (#286) against
//! intellect (2), branching on the controller's result —
//! `on_success` heals 1 damage, `on_fail` deals 1 damage, each to the chosen
//! investigator at the controller's location
//! ([`InvestigatorTarget::chosen_at_your_location`], #349). Composed entirely
//! from existing primitives — no native effect, no engine work. The first
//! `Effect::SkillTest` initiated from an *activated* ability (every prior
//! caller is a Revelation/forced effect); `activate_ability` ends in
//! `apply_effect`, so the test suspends at the commit window and resumes
//! through the same `drive_skill_test` path regardless of origin.
//!
//! # The target is chosen *inside* the post-test branch — exact in solo
//!
//! The printed card chooses the target **before** the test ("Choose … and
//! test"); this impl chooses inside whichever branch runs (`on_success` /
//! `on_fail`). **Exact under today's solo engine:** with one investigator at
//! your location the `Chosen` choice auto-binds (count 1, no suspend), so
//! choosing before vs. after the test is indistinguishable. The divergence
//! only appears with 2+ investigators at your location (multiplayer), where a
//! post-test choice would let the player pick the target after seeing the
//! result — strictly more information than the card grants.
//!
//! TODO(#359): bind the chosen investigator before the test (needs the
//! binding to survive the `Effect::SkillTest` suspend/resume) once
//! multiplayer makes the ordering observable. Mirrors Machete's #300
//! deferral.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{activated, deal_damage, heal_damage, skill_test, Ability, InvestigatorTarget};

/// `ArkhamDB` code for Medical Texts (original-Core printing).
pub const CODE: &str = "01035";

/// Medical Texts' `[action]` intellect(2) test: heal 1 damage on success,
/// deal 1 damage on failure, to a chosen investigator at your location.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![],
        skill_test(
            SkillKind::Intellect,
            2,
            Some(heal_damage(
                InvestigatorTarget::chosen_at_your_location(),
                1,
            )),
            Some(deal_damage(
                InvestigatorTarget::chosen_at_your_location(),
                1,
            )),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_action_intellect_test_branching_heal_or_damage() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert!(
            abilities[0].costs.is_empty(),
            "Medical Texts' action has no exhaust/uses cost",
        );

        let Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } = &abilities[0].effect
        else {
            panic!("expected Effect::SkillTest");
        };
        assert_eq!(*skill, SkillKind::Intellect);
        assert_eq!(*difficulty, 2);

        let target = InvestigatorTarget::chosen_at_your_location();
        assert_eq!(
            on_success.as_deref(),
            Some(&heal_damage(target, 1)),
            "success heals 1 damage from the chosen investigator",
        );
        assert_eq!(
            on_fail.as_deref(),
            Some(&deal_damage(target, 1)),
            "failure deals 1 damage to the chosen investigator",
        );
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
