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
//! The unrevealed back:
//!
//! ```text
//! The entrance to the Parlor is blocked by a darkly glowing unfathomable
//! barrier. You cannot move into the Parlor.
//! ```
//!
//! # Module gap
//!
//! **The front's Lita grant is not implemented.** The card prints two front
//! clauses and one on its back; the grant is the one gap, tracked rather than
//! approximated:
//!
//! - `TODO(#772)`: the Lita Chantler grant — *"While Lita Chantler is not
//!   controlled by a player, she gains: '\[action\]: **Parley.** Test
//!   \[intellect\] (4). If you succeed, take control of Lita Chantler.'"* An
//!   ability one card grants to another, conditional on control, which the DSL
//!   has no shape for.
//!
//! That gap does not touch the Resign: it is a front-side `[action]` with no
//! interaction with the grant, so shipping it alone resolves correctly rather
//! than approximately.
//!
//! # The back: the barrier
//!
//! **The card is double-sided**, and the barrier is printed on the side in
//! effect while the location is unrevealed — so the barrier is a property of
//! the Parlor being **unrevealed**, and act 01109b lifts it by revealing: *"The
//! barrier blocking passage into the parlor has vanished. Reveal the Parlor."*
//! Nothing on the card conditions it, so [`back_abilities`] is a plain
//! unconditional `Trigger::Constant` + `Effect::Restrict` — a bare restriction,
//! never an `Effect::If`. The engine picks the side
//! (`game_core::engine::abilities_in_effect`); this module only declares what
//! each side says.
//!
//! **The restriction is investigator-side only**, and the card's only
//! `ArkhamDB` ruling is why:
//!
//! > **Q:** Can enemies move into Parlor even when investigators are blocked by
//! > the barrier? **A:** Yes; in The Gathering scenario, enemies can move into
//! > The Parlor even when the investigators are blocked by the barrier.
//! > (March 2024)
//!
//! (<https://arkhamdb.com/card/01115>) — which is why
//! `Restriction::InvestigatorMovementBlocked` is a second restriction beside
//! Barricade 01038's `EnemyMovementBlocked` rather than one shared block: the
//! Parlor stops investigators and not enemies, Barricade stops enemies and not
//! investigators.
//!
//! # Designator and effect
//!
//! **Designator: Resign**, declared on the trigger, which is what exempts the
//! activation from attacks of opportunity — `glossary/Attack_of_Opportunity.md`
//! exempts *"an action other than to **fight**, to **evade**, or to activate a
//! **parley** or **resign** ability"* — and which **performs the elimination**
//! (#644, #805). Nothing is printed beside it: the designator is the whole
//! ability, as Machete 01020's **Fight** is the whole of its.
//!
//! The quoted sentence is flavour: `glossary/Resign.md` gives the ability its
//! whole meaning — *"When an investigator resigns, the investigator is
//! eliminated by resignation … An investigator who resigns is not considered to
//! have been defeated."* No cost beyond the one action, and no eligibility
//! clause: the reachability rule (an investigator may use a scenario card's
//! ability while at its location — ADR 0010) is the whole restriction, and it is
//! the engine's, not the card's.

use card_dsl::dsl::{
    activated_as, constant, restrict, seq, Ability, ActionDesignator, Restriction,
};

/// `ArkhamDB` code for the Parlor.
pub const CODE: &str = "01115";

/// The Parlor's front — its `[action]` **Resign**. In effect while the
/// location is revealed.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated_as(ActionDesignator::Resign, 1, vec![], seq([]))]
}

/// The Parlor's unrevealed back — the barrier. In effect while the location is
/// unrevealed, which is exactly until act 01109b reveals it.
#[must_use]
pub fn back_abilities() -> Vec<Ability> {
    vec![constant(restrict(Restriction::InvestigatorMovementBlocked))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{ActionDesignator, Effect, Restriction, Trigger};

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
        // The designator performs the resignation; nothing is printed beside it.
        assert_eq!(abilities[0].effect, Effect::Seq(vec![]));
    }

    #[test]
    fn back_abilities_are_one_constant_investigator_movement_block() {
        let abilities = super::back_abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Constant);
        assert!(abilities[0].costs.is_empty());
        // A bare `Restrict`, never an `Effect::If`: *"You cannot move into the
        // Parlor"* is unqualified, and the reveal is what lifts it. And
        // investigator-side, not enemy-side — 01115's ruling has enemies moving
        // into the Parlor while investigators are blocked.
        assert_eq!(
            abilities[0].effect,
            Effect::Restrict(Restriction::InvestigatorMovementBlocked)
        );
    }
}
