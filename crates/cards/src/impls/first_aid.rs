//! First Aid (Guardian item asset, 01019).
//!
//! ```text
//! Uses (3 supplies). If First Aid has no supplies, discard it.
//! [action] Spend 1 supply: Heal 1 damage or horror from an investigator
//!   at your location.
//! ```
//!
//! One activated ability: an action paying 1 supply (`Cost::SpendUses`) to
//! heal 1 damage **or** horror from a chosen investigator at the controller's
//! location. The damage-or-horror choice is an `Effect::ChooseOne` over the
//! two `Effect::Heal` branches (#302), each targeting the keystone's
//! `InvestigatorTarget::Chosen(At(your location))` (#349).
//!
//! **The branch labels split one printed sentence.** 01019 prints the two modes
//! in a single clause, verbatim (`data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > \[action\] Spend 1 supply: Heal 1 damage or horror from an investigator
//! > at your location.
//!
//! so unlike a card that prints its options as bullets, labelling means
//! dividing that clause — *"Heal 1 damage from an investigator at your
//! location"* and *"Heal 1 horror from an investigator at your location"*.
//! The division is the sentence's own *"damage or horror"*; nothing is added.
//! 01019 has no rulings (`data/arkhamdb-faq/no-rulings.txt`). The `Uses (3
//! supplies)` pool and the "if no supplies, discard it" depletion-discard are
//! corpus metadata (`CardKind::Asset.uses` + `Uses.discard_when_empty`,
//! pipeline-parsed, #302) — `abilities()` declares only the action; the engine
//! discards the asset automatically when the last supply is spent.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{
    activated, choose_one, heal_damage, heal_horror, Ability, Cost, InvestigatorTarget,
};

/// `ArkhamDB` code for First Aid (original-Core printing).
pub const CODE: &str = "01019";

/// Label for the damage mode of the printed *"Heal 1 damage or horror"* choice.
const HEAL_DAMAGE_LABEL: &str = "Heal 1 damage from an investigator at your location";
/// Label for the horror mode of the same printed clause.
const HEAL_HORROR_LABEL: &str = "Heal 1 horror from an investigator at your location";

/// First Aid's `[action] Spend 1 supply: heal 1 damage or horror` ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Supplies,
            count: 1,
        }],
        // Labels split the one printed sentence quoted in the module doc: the
        // card names both modes in a single "damage or horror" clause, so each
        // branch's label is that clause narrowed to its own mode.
        choose_one([
            (
                HEAL_DAMAGE_LABEL,
                heal_damage(InvestigatorTarget::chosen_at_your_location(), 1),
            ),
            (
                HEAL_HORROR_LABEL,
                heal_horror(InvestigatorTarget::chosen_at_your_location(), 1),
            ),
        ]),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn one_action_ability_spending_a_supply_to_choose_a_heal() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::Activated {
                action_cost: 1,
                designator: None
            }
        );
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            }]
        );
        let Effect::ChooseOne(branches) = &abilities[0].effect else {
            panic!("expected ChooseOne (damage or horror)");
        };
        assert_eq!(branches.len(), 2);
        let expected_target = InvestigatorTarget::chosen_at_your_location();
        assert_eq!(
            branches[0].effect,
            heal_damage(expected_target, 1),
            "branch 0 heals 1 damage from an investigator at your location",
        );
        assert_eq!(
            branches[1].effect,
            heal_horror(expected_target, 1),
            "branch 1 heals 1 horror from an investigator at your location",
        );
        assert_eq!(branches[0].label, HEAL_DAMAGE_LABEL);
        assert_eq!(branches[1].label, HEAL_HORROR_LABEL);
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
