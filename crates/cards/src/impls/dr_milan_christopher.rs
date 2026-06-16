//! Dr. Milan Christopher (Seeker ally asset, 01033).
//!
//! ```text
//! Ally. Miskatonic.
//! You get +1 [intellect].
//! [reaction] After you successfully investigate: Gain 1 resource.
//! ```
//!
//! Two abilities: a `Constant` +1 intellect while in play, and an
//! `OnEvent` reaction at the after-successful-investigate window (C6a
//! #241) that gains the controller 1 resource. "after **you**
//! investigate" is enforced by the window being controller-scoped, so the
//! pattern is bare and the effect's `You` resolves to the controller.
//!
//! # Ally-soak gap
//!
//! Card metadata gives Dr. Milan `health: 1, sanity: 2` — ally
//! damage/horror soak, not a max-stat boost on the controller. The DSL
//! doesn't model soak yet (#44, shared with Holy Rosary / Beat Cop), so
//! this impl ships only the +1 intellect and the reaction; the card is
//! mechanically weaker than printed until the soak primitive lands.

use card_dsl::dsl::{
    constant, gain_resources, modify, reaction_on_event, Ability, EventPattern, EventTiming,
    InvestigatorTarget, ModifierScope, Stat,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01033";

/// +1 intellect while in play, and "after you successfully investigate,
/// gain 1 resource."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        constant(modify(Stat::Intellect, 1, ModifierScope::WhileInPlay)),
        reaction_on_event(
            EventPattern::SuccessfullyInvestigated,
            EventTiming::After,
            gain_resources(InvestigatorTarget::You, 1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, InvestigatorTarget, ModifierScope, Stat, Trigger,
    };

    #[test]
    fn abilities_are_constant_intellect_and_after_investigate_reaction() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);

        assert_eq!(abilities[0].trigger, Trigger::Constant);
        assert!(matches!(
            abilities[0].effect,
            Effect::Modify {
                stat: Stat::Intellect,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
            }
        ));

        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::SuccessfullyInvestigated,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Reaction,
            },
        );
        assert_eq!(
            abilities[1].effect,
            Effect::GainResources {
                target: InvestigatorTarget::You,
                amount: 1,
            },
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
