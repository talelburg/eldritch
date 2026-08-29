//! Parlor (The Gathering location, 01115).
//!
//! ```text
//! Shroud: 2. Clues: 0.
//! [action] Resign. "This is too much for me!" You run out the front door,
//! fleeing in panic.
//! While Lita Chantler is not controlled by a player, she gains: "[action]:
//! Parley. Test [intellect] (4). If you succeed, take control of Lita
//! Chantler."
//! ```
//!
//! # Module gap
//!
//! **Only the Resign clause is implemented.** The card prints three; the other
//! two are unimplemented and tracked, not approximated:
//!
//! - `TODO(#772)`: the Lita Chantler grant — *"While Lita Chantler is not
//!   controlled by a player, she gains: '\[action\]: **Parley.** Test
//!   \[intellect\] (4). If you succeed, take control of Lita Chantler.'"* An
//!   ability one card grants to another, conditional on control, which the DSL
//!   has no shape for.
//! - `TODO(#774)`: the back side's barrier — *"The entrance to the Parlor is
//!   blocked by a darkly glowing unfathomable barrier. You cannot move into the
//!   Parlor."* The card's only `ArkhamDB` ruling belongs to that clause, not to
//!   this one: *"**Q:** Can enemies move into Parlor even when investigators are
//!   blocked by the barrier? **A:** Yes; in The Gathering scenario, enemies can
//!   move into The Parlor even when the investigators are blocked by the
//!   barrier."* (<https://arkhamdb.com/card/01115>)
//!
//! Neither gap touches the Resign: it is a front-side `[action]` with no
//! interaction with either clause, so shipping it alone resolves correctly
//! rather than approximately.
//!
//! # Designator and effect
//!
//! **Designator: Resign**, declared on the trigger (`ActionDesignator::Resign`,
//! #696), which is what exempts the activation from attacks of opportunity —
//! `glossary/Attack_of_Opportunity.md` exempts *"an action other than to
//! **fight**, to **evade**, or to activate a **parley** or **resign**
//! ability"*. The elimination itself is the effect's (`Effect::Resign`, #644),
//! the same split Machete 01020 has between its **Fight** designator and its
//! `Effect::Fight`.
//!
//! The quoted sentence is flavour: `glossary/Resign.md` gives the ability its
//! whole meaning — *"When an investigator resigns, the investigator is
//! eliminated by resignation … An investigator who resigns is not considered to
//! have been defeated."* No cost beyond the one action, and no eligibility
//! clause: the reachability rule (an investigator may use a scenario card's
//! ability while at its location — ADR 0010) is the whole restriction, and it is
//! the engine's, not the card's.

use card_dsl::dsl::{activated_as, resign, Ability, ActionDesignator};

/// `ArkhamDB` code for the Parlor.
pub const CODE: &str = "01115";

/// The Parlor's `[action]` **Resign**.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(ActionDesignator::Resign, 1, vec![], resign())]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Effect, Trigger};

    #[test]
    fn abilities_are_one_action_costed_resign() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::Activated {
                action_cost: 1,
                designator: Some(ActionDesignator::Resign),
            }
        );
        assert!(abilities[0].costs.is_empty());
        assert_eq!(abilities[0].effect, Effect::Resign);
    }
}
