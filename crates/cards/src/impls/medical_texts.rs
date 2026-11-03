//! Medical Texts (Seeker item asset, 01035).
//!
//! ```text
//! [action] Choose an investigator at your location and test [intellect] (2).
//!   If you succeed, heal 1 damage from that investigator. If you fail, deal
//!   1 damage to that investigator.
//! ```
//!
//! No rulings: 01035 is listed in `data/arkhamdb-faq/no-rulings.txt`.
//!
//! One `[action]` ability (no exhaust, no uses), composed entirely from DSL
//! primitives. An [`Effect::ChooseInvestigator`](card_dsl::dsl::Effect::ChooseInvestigator)
//! over the controller's location picks the target first, then an
//! [`Effect::SkillTest`](card_dsl::dsl::Effect::SkillTest) against intellect
//! (2) branches on the result: `on_success` heals 1 damage from, and `on_fail`
//! deals 1 damage to, [`InvestigatorTarget::That`], the investigator picked.
//!
//! # The target is chosen before the test
//!
//! The printed card chooses, then tests, so the pick is made before anyone
//! commits a card or reveals a token, and the result branch reads the same
//! pick without asking again. Every investigator at the location is offered,
//! the controller included, damaged or not: a failure deals damage, so any of
//! them can have their state changed (Rules Reference, *Target*: *"A card is
//! not an eligible target for an ability if the resolution of that ability's
//! effect could not change the target's state."*). A success on an undamaged
//! investigator heals nothing, and the test still resolves.
//!
//! Solo, the one investigator at the location is picked without a prompt
//! outside interactive play; under `interactive_acknowledge` the lone pick
//! surfaces as a one-option prompt, like every other lone pick.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{self, Ability, InvestigatorTarget, LocationSet};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Medical Texts (original-Core printing).
pub const CODE: &str = "01035";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Medical Texts' `[action]`: choose an investigator at your location, then
/// an intellect(2) test that heals 1 damage from them on success and deals
/// them 1 damage on failure.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated(
        1,
        vec![],
        dsl::choose_investigator(
            LocationSet::Here,
            dsl::skill_test(
                SkillKind::Intellect,
                2,
                Some(dsl::heal_damage(InvestigatorTarget::That, 1)),
                Some(dsl::deal_damage(InvestigatorTarget::That, 1u8)),
            ),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    use super::*;

    #[test]
    fn one_action_choosing_a_target_here_then_an_intellect_test() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 },);
        assert_eq!(abilities[0].designator, None);
        assert!(
            abilities[0].costs.is_empty(),
            "Medical Texts' action has no exhaust/uses cost",
        );

        let Effect::ChooseInvestigator { scope, then } = &abilities[0].effect else {
            panic!("expected Effect::ChooseInvestigator");
        };
        assert_eq!(*scope, LocationSet::Here, "an investigator at your location");
        let Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } = then.as_ref()
        else {
            panic!("expected Effect::SkillTest under the choice");
        };
        assert_eq!(*skill, SkillKind::Intellect);
        assert_eq!(*difficulty, 2);

        assert_eq!(
            on_success.as_deref(),
            Some(&dsl::heal_damage(InvestigatorTarget::That, 1)),
            "success heals 1 damage from that investigator",
        );
        assert_eq!(
            on_fail.as_deref(),
            Some(&dsl::deal_damage(InvestigatorTarget::That, 1u8)),
            "failure deals 1 damage to that investigator",
        );
    }
}
