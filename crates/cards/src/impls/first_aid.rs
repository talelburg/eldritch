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
//! `InvestigatorTarget::Chosen(At(your location))` (#349). The `Uses (3
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

/// First Aid's `[action] Spend 1 supply: heal 1 damage or horror` ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Supplies,
            count: 1,
        }],
        choose_one([
            heal_damage(InvestigatorTarget::chosen_at_your_location(), 1),
            heal_horror(InvestigatorTarget::chosen_at_your_location(), 1),
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
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
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
            branches[0],
            heal_damage(expected_target, 1),
            "branch 0 heals 1 damage from an investigator at your location",
        );
        assert_eq!(
            branches[1],
            heal_horror(expected_target, 1),
            "branch 1 heals 1 horror from an investigator at your location",
        );
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
