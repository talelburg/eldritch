//! Flashlight (neutral tool asset, 01087).
//!
//! ```text
//! Uses (3 supplies).
//! [action] Spend 1 supply: Investigate. Your location gets -2 shroud for
//!   this investigation.
//! ```
//!
//! One activated ability: an action paying 1 supply (`Cost::SpendUses`) to
//! Investigate the controller's location with its shroud reduced by 2 for
//! this investigation.
//!
//! **Designator: Investigate** — the bold word, which **performs the
//! investigation** (#805) carrying the `-2` as its `shroud_modifier` (#313).
//! The modifier lowers the location *difficulty* (clamped at 0), not the
//! investigator's total, and the investigation is the same primary the basic
//! investigate action runs, so a success discovers a clue exactly as that does.
//!
//! **Investigate** is not on the attack-of-opportunity exempt list
//! (`glossary/Attack_of_Opportunity.md` names only **fight**, **evade**,
//! **parley** and **resign**), so activating this `[action]` while engaged with
//! a ready enemy provokes one — again exactly as the basic action does. Shipped
//! engine-wide in #361, exercised in
//! `crates/cards/tests/activate_ability_aoo.rs`.
//!
//! The `Uses (3 supplies)` pool is corpus metadata (`CardKind::Asset.uses`,
//! pipeline-parsed with `discard_when_empty: false` — Flashlight's printed
//! text has no depletion-discard clause, so it stays in play at 0 supplies,
//! unlike First Aid). `abilities()` declares only the action.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated_as, investigate, seq, Ability, Cost};

/// `ArkhamDB` code for Flashlight (original-Core printing).
pub const CODE: &str = "01087";

/// Flashlight's `[action] Spend 1 supply: Investigate with -2 shroud` ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(
        investigate(-2i8),
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Supplies,
            count: 1,
        }],
        seq([]),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{ActionDesignator, Effect, IntExpr, Trigger};

    #[test]
    fn one_action_ability_spending_a_supply_to_investigate_minus_two_shroud() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::Activated {
                action_cost: 1,
                designator: Some(ActionDesignator::Investigate {
                    shroud_modifier: IntExpr::Lit(-2),
                }),
            }
        );
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            }]
        );
        // The designator performs the investigation; nothing is printed beside
        // it (#805).
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
