//! Vicious Blow (Guardian skill, 01025).
//!
//! ```text
//! Practiced.
//! 1 combat icon.
//! If this skill test is successful during an attack, that attack deals
//! +1 damage.
//! ```
//!
//! The 1 combat icon is part of the card's printed metadata
//! (`SkillIcons { combat: 1, .. }`), not part of `abilities()` — the
//! icon-contribution path lives in the skill-test commit window
//! (`finish_skill_test` reads icons via the registry's `metadata_for`),
//! not through the DSL.
//!
//! `abilities()` describes only the triggered effect: a
//! [`Trigger::OnCommit`] running [`Effect::BoostAttackDamage`], gated by an
//! [`if_`] over [`Condition::SkillTestKind`] of [`SkillTestKind::Fight`].
//!
//! The **"during an attack"** qualifier is expressed by that kind gate —
//! symmetric to Deduction's "while investigating"
//! ([`Condition::SkillTestKind`] of [`SkillTestKind::Investigate`]). Every
//! attack in the engine, whether the Fight *action* or an `Effect::Fight`
//! weapon, runs a [`SkillTestKind::Fight`] test, so the gate captures
//! exactly "an attack" and nothing else. Gating the *accumulate* (rather
//! than relying on the Fight follow-up being the only reader of
//! [`InFlightSkillTest::bonus_attack_damage`]) keeps the buff from leaking
//! into a non-attack test even if a second reader is added later.
//!
//! The **"if successful"** qualifier stays intrinsic to
//! [`Effect::BoostAttackDamage`] (see #307 / PR #308): the bonus is
//! consumed by the Fight follow-up, which deals damage only on success.
//! It cannot be card-expressed at commit because the outcome is not yet
//! known — and `OnCommit` (not `OnSkillTestResolution`) is required because
//! the follow-up deals the attack's damage *during* resolution, before the
//! resolution trigger fires.
//!
//! [`Trigger::OnCommit`]: card_dsl::dsl::Trigger::OnCommit
//! [`Effect::BoostAttackDamage`]: card_dsl::dsl::Effect::BoostAttackDamage
//! [`Condition::SkillTestKind`]: card_dsl::dsl::Condition::SkillTestKind
//! [`SkillTestKind::Fight`]: card_dsl::dsl::SkillTestKind::Fight
//! [`SkillTestKind::Investigate`]: card_dsl::dsl::SkillTestKind::Investigate
//! [`if_`]: card_dsl::dsl::if_
//! [`InFlightSkillTest::bonus_attack_damage`]: game_core::state::InFlightSkillTest::bonus_attack_damage

use card_dsl::dsl::{boost_attack_damage, if_, on_commit, Ability, Condition, SkillTestKind};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01025";

/// On commit to a Fight test, add +1 to the attack's damage (consumed by
/// the Fight follow-up, which deals damage only on success).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_commit(if_(
        Condition::SkillTestKind(SkillTestKind::Fight),
        boost_attack_damage(1),
    ))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Condition, Effect, SkillTestKind, Trigger};

    #[test]
    fn abilities_are_one_on_commit_attack_buff() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnCommit);
        // If(SkillTestKind(Fight), BoostAttackDamage(1)) — "during an attack".
        let Effect::If {
            condition, then, ..
        } = &abilities[0].effect
        else {
            panic!("expected Effect::If, got {:?}", abilities[0].effect);
        };
        assert_eq!(condition, &Condition::SkillTestKind(SkillTestKind::Fight));
        assert_eq!(**then, Effect::BoostAttackDamage(1));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
