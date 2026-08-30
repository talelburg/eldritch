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
//! # The front's second clause: Lita's Parley
//!
//! *"While Lita Chantler is not controlled by a player, she gains: '\[action\]:
//! **Parley.** Test \[intellect\] (4). If you succeed, take control of Lita
//! Chantler.'"*
//!
//! **The ability is Lita's, not the Parlor's.** `glossary/Gains.md`: *"If a card
//! gains a characteristic (such as an icon, a trait, a keyword, or ability text),
//! the card functions as if it possesses the gained characteristic."* — so an
//! investigator in the Parlor activates it *on Lita*, and the source it names is
//! her instance in `cards_at_location`. What the Parlor prints is the **grant**:
//! a `Trigger::Constant` `Effect::Grant`, inspected by the engine's grant sweep
//! and never executed (ADR 0014).
//!
//! Three things about the shape are load-bearing:
//!
//! - **The condition is a field on the `Grant`, never an `Effect::If` around
//!   it.** A gated grant is silently invisible to the sweep, exactly as a gated
//!   `Effect::Modify` is invisible to the modifier sweep (#679). The test below
//!   pins the bare shape, as the back-side `Restrict` test already does.
//! - **The gate is board-global.** *"not controlled by a player"* asks about
//!   Lita and about nobody's "you" — which is what lets it be answered at all,
//!   since the sweep evaluates a grant's condition against the *recipient's*
//!   controller and Lita has none until the Parley succeeds. Hence
//!   `Condition::ControlStatus` rather than a card-local native tag, whose
//!   predicate could not be asked without a controller to bind.
//! - **The two grants are complements, and nothing enforces it.** Lita's own
//!   card grants her a different pair *while* a player controls her (#773), so
//!   exactly one of the two applies at a time. That is the two cards agreeing,
//!   not a property of the mechanism.
//!
//! `[action]` **Parley** needs no new action type: #696 shipped
//! `ActionDesignator::Parley` and its attack-of-opportunity exemption, and
//! `glossary/Parley.md` says the designator performs nothing of its own — *"Some
//! abilities are identified with a **Parley** action designator. Such abilities
//! are initiated using the 'Activate' action."* — so the whole ability is the
//! test and its success clause.
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

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    activated_as, constant, control_status, grant, restrict, seq, skill_test, take_control,
    Ability, ActionDesignator, ControlStatus, GrantTarget, Restriction,
};

/// `ArkhamDB` code for the Parlor.
pub const CODE: &str = "01115";

/// `ArkhamDB` code for Lita Chantler, the card this location grants to.
const LITA_CHANTLER: &str = "01117";

/// The Parlor's front — its `[action]` **Resign**, and the Parley it grants
/// Lita Chantler while nobody controls her. In effect while the location is
/// revealed.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        activated_as(ActionDesignator::Resign, 1, vec![], seq([])),
        constant(grant(
            GrantTarget::Card(LITA_CHANTLER.to_owned()),
            Some(control_status(LITA_CHANTLER, ControlStatus::ByNoPlayer)),
            vec![lita_parley()],
        )),
    ]
}

/// The granted ability, verbatim: *"\[action\]: **Parley.** Test
/// \[intellect\] (4). If you succeed, take control of Lita Chantler."*
///
/// One action, no payment cost, and no eligibility clause — the reachability
/// rule (an investigator may use a scenario card's ability while at its
/// location, ADR 0010) is the whole restriction, and it is the engine's. The
/// success clause is the test's `on_success`, so a failed Parley leaves Lita
/// where she is and costs only the action.
fn lita_parley() -> Ability {
    activated_as(
        ActionDesignator::Parley,
        1,
        vec![],
        skill_test(
            SkillKind::Intellect,
            4,
            Some(take_control(LITA_CHANTLER)),
            None,
        ),
    )
}

/// The Parlor's unrevealed back — the barrier. In effect while the location is
/// unrevealed, which is exactly until act 01109b reveals it.
#[must_use]
pub fn back_abilities() -> Vec<Ability> {
    vec![constant(restrict(Restriction::InvestigatorMovementBlocked))]
}

#[cfg(test)]
mod tests {
    use card_dsl::card_data::SkillKind;
    use card_dsl::dsl::{
        ActionDesignator, Condition, ControlStatus, Effect, GrantTarget, Restriction, Trigger,
    };

    #[test]
    fn abilities_are_one_action_costed_resign_then_the_lita_grant() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
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
        assert_eq!(abilities[1].trigger, Trigger::Constant);
    }

    /// **A bare `Grant`, never an `Effect::If`.** The grant sweep matches only
    /// an unwrapped `Effect::Grant` under `Trigger::Constant`, so a condition
    /// hoisted into an `Effect::If` would make the Parley silently unavailable —
    /// the same trap a gated `Effect::Modify` falls into (#679). The gate is the
    /// variant's own `condition` field, and it is board-global: it asks about
    /// Lita, not about any investigator's "you", which is what lets it be
    /// answered while nobody controls her. The status is the **negative** one —
    /// 01117's own grant asks for the other, and the two are complements.
    #[test]
    fn the_lita_grant_is_bare_and_gated_on_nobody_controlling_her() {
        let abilities = super::abilities();
        let Effect::Grant {
            ref to,
            ref condition,
            ref abilities,
        } = abilities[1].effect
        else {
            panic!(
                "the second ability is the Lita grant, got {:?}",
                abilities[1]
            );
        };
        assert_eq!(*to, GrantTarget::Card("01117".to_owned()));
        assert_eq!(
            *condition,
            Some(Condition::ControlStatus {
                code: "01117".to_owned(),
                status: ControlStatus::ByNoPlayer,
            }),
        );
        assert_eq!(abilities.len(), 1, "the card grants exactly one ability");
    }

    /// *"\[action\]: **Parley.** Test \[intellect\] (4). If you succeed, take
    /// control of Lita Chantler."* — one action, no payment cost, an intellect
    /// (4) test, and the take-control on the **success** side alone.
    #[test]
    fn the_granted_ability_is_a_one_action_parley_intellect_four_take_control() {
        let parley = super::lita_parley();
        assert_eq!(
            parley.trigger,
            Trigger::Activated {
                action_cost: 1,
                designator: Some(ActionDesignator::Parley),
            }
        );
        assert!(parley.costs.is_empty());
        assert!(
            parley.usage_limit.is_none(),
            "the card prints no limit, and a granted ability has no counter to key one by",
        );
        assert_eq!(
            parley.effect,
            Effect::SkillTest {
                skill: SkillKind::Intellect,
                difficulty: 4,
                on_success: Some(Box::new(Effect::TakeControl {
                    code: "01117".to_owned(),
                })),
                on_fail: None,
            },
        );
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
