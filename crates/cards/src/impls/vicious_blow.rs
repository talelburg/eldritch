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
//! `abilities()` describes only the triggered effect: a success-gated
//! [result modifier](card_dsl::dsl::ResultContribution::Modifies) running
//! [`Effect::BoostAttackDamage`], gated by an [`if_`] over
//! [`Condition::SkillTestKind`] of [`SkillTestKind::Fight`].
//!
//! It is a modifier because the ruling says so: *"“that attack” is given +1
//! damage, rather than Vicious Blow itself dealing 1 damage"*
//! (<https://arkhamdb.com/card/01025>). So it applies at the head of ST.7,
//! before the Fight follow-up deals the attack's damage (**Result modifier** in
//! `GLOSSARY.md`). The trigger's `Success` gate is the **"if this skill test is
//! successful"** qualifier.
//!
//! The **"during an attack"** qualifier is expressed by the kind gate —
//! symmetric to Deduction 01039's "while investigating"
//! ([`Condition::SkillTestKind`] of [`SkillTestKind::Investigate`]). Every
//! attack in the engine, whether the Fight *action* or a designated **Fight**
//! weapon, runs a [`SkillTestKind::Fight`] test, so the gate captures
//! exactly "an attack" and nothing else. Gating the *accumulate* (rather
//! than relying on the Fight follow-up being the only reader of
//! [`InFlightSkillTest::bonus_attack_damage`]) keeps the buff from leaking
//! into a non-attack test even if a second reader is added later.
//!
//! [`Effect::BoostAttackDamage`]: card_dsl::dsl::Effect::BoostAttackDamage
//! [`Condition::SkillTestKind`]: card_dsl::dsl::Condition::SkillTestKind
//! [`SkillTestKind::Fight`]: card_dsl::dsl::SkillTestKind::Fight
//! [`SkillTestKind::Investigate`]: card_dsl::dsl::SkillTestKind::Investigate
//! [`if_`]: card_dsl::dsl::if_
//! [`InFlightSkillTest::bonus_attack_damage`]: game_core::state::InFlightSkillTest::bonus_attack_damage

use card_dsl::dsl::{self, Ability, Condition, SkillTestKind, TestOutcome};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01025";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// On a successful Fight test, add +1 to the attack's damage before the Fight
/// follow-up deals it.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::modifies_result(
        TestOutcome::Success,
        dsl::if_(
            Condition::SkillTestKind(SkillTestKind::Fight),
            dsl::boost_attack_damage(1),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Condition, Effect, ResultContribution, ResultController, SkillTestKind, TestOutcome, Trigger,
    };

    #[test]
    fn abilities_are_one_success_modifier_attack_buff() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnSkillTestResolution {
                outcome: TestOutcome::Success,
                contributes: ResultContribution::Modifies,
                controller: ResultController::Performer,
            },
        );
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
}
