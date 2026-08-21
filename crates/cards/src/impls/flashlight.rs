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
//! this investigation. The `-2` is an
//! [`Effect::Investigate`](card_dsl::dsl::Effect::Investigate) `shroud_modifier`
//! (#313) — the Investigate mirror of `Effect::Fight`: it lowers the
//! location *difficulty* (clamped at 0), not the investigator's total, and
//! reuses the base Investigate follow-up so a success discovers a clue. The
//! `Uses (3 supplies)` pool is corpus metadata (`CardKind::Asset.uses`,
//! pipeline-parsed with `discard_when_empty: false` — Flashlight's printed
//! text has no depletion-discard clause, so it stays in play at 0 supplies,
//! unlike First Aid). `abilities()` declares only the action.
//!
//! **Designator: Investigate** (`ActionDesignator::Investigate`, #696) — the
//! bold word above the effect. **Investigate** is not on the attack-of-
//! opportunity exempt list (`glossary/Attack_of_Opportunity.md` names only
//! **fight**, **evade**, **parley** and **resign**), so activating this
//! `[action]` while engaged with a ready enemy provokes one — exactly as the
//! basic investigate action does. Shipped engine-wide in #361, exercised in
//! `crates/cards/tests/activate_ability_aoo.rs`.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{activated_as, investigate, Ability, ActionDesignator, Cost};

/// `ArkhamDB` code for Flashlight (original-Core printing).
pub const CODE: &str = "01087";

/// Flashlight's `[action] Spend 1 supply: Investigate with -2 shroud` ability.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(
        ActionDesignator::Investigate,
        1,
        vec![Cost::SpendUses {
            kind: UseKind::Supplies,
            count: 1,
        }],
        investigate(-2i8),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, IntExpr, Trigger};

    #[test]
    fn one_action_ability_spending_a_supply_to_investigate_minus_two_shroud() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::Activated {
                action_cost: 1,
                designator: Some(ActionDesignator::Investigate),
            }
        );
        assert_eq!(
            abilities[0].costs,
            vec![Cost::SpendUses {
                kind: UseKind::Supplies,
                count: 1,
            }]
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::Investigate {
                shroud_modifier: IntExpr::Lit(-2),
            }
        ));
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
