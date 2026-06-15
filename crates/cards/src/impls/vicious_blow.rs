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
//! [`Trigger::OnCommit`] running [`Effect::BoostAttackDamage`]. The
//! "during an attack" and "if successful" qualifiers are **intrinsic** to
//! that primitive (see #307 / PR #308): the bonus accumulates onto the
//! in-flight test at commit, and only a *Fight* follow-up reads it, dealing
//! damage only on *success* — so committing to a non-attack test, or to a
//! failed one, is a harmless no-op. No kind-narrowing [`Condition`] (unlike
//! Deduction) is needed.
//!
//! [`Trigger::OnCommit`]: card_dsl::dsl::Trigger::OnCommit
//! [`Effect::BoostAttackDamage`]: card_dsl::dsl::Effect::BoostAttackDamage
//! [`Condition`]: card_dsl::dsl::Condition

use card_dsl::dsl::{boost_attack_damage, on_commit, Ability};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01025";

/// On commit, add +1 to the attack's damage (resolves only for a
/// successful Fight test — intrinsic to `BoostAttackDamage`).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_commit(boost_attack_damage(1))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn abilities_are_one_on_commit_attack_buff() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnCommit);
        assert_eq!(abilities[0].effect, Effect::BoostAttackDamage(1));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
