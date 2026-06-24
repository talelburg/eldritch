//! Roland Banks (Guardian investigator, 01001).
//!
//! ```text
//! Roland Banks. The Fed.
//! Agency. Detective.
//! Willpower 3. Intellect 3. Combat 4. Agility 2.
//! Health 9. Sanity 5.
//!
//! [reaction] After you defeat an enemy: Discover 1 clue at your
//! location. (Limit once per round.)
//! [elder_sign] effect: +1 for each clue on your location.
//! ```
//!
//! # Scope
//!
//! `abilities()` ships both halves: the `[reaction]` and the
//! `[elder_sign]`. The elder-sign is a [`Trigger::ElderSign`] carrying
//! `IntExpr::Count(Quantity::CluesAtControllerLocation)` — "+1 for each clue
//! on your location" — which the skill-test resolution adds to the total when
//! Roland's elder-sign token is drawn (#118). Reached via the investigator-card
//! bridge (`Investigator.card_code`); sunset by #448.
//!
//! The reaction compiles to a [`Trigger::OnEvent`] with the
//! [`EventPattern::EnemyDefeated`] pattern narrowed by
//! `by_controller: true` (Roland's "After **you** defeat an enemy")
//! at [`EventTiming::After`], discovering 1 clue at
//! [`LocationTarget::YourLocation`]. The "Limit once per round"
//! clause attaches as a [`UsageLimit`] with `count: 1, period:
//! UsagePeriod::Round`.
//!
//! [`Trigger::OnEvent`]: card_dsl::dsl::Trigger::OnEvent
//! [`Trigger::ElderSign`]: card_dsl::dsl::Trigger::ElderSign
//! [`EventPattern::EnemyDefeated`]: card_dsl::dsl::EventPattern::EnemyDefeated
//! [`EventTiming::After`]: card_dsl::dsl::EventTiming::After
//! [`LocationTarget::YourLocation`]: card_dsl::dsl::LocationTarget::YourLocation
//! [`UsageLimit`]: card_dsl::dsl::UsageLimit

use card_dsl::dsl::{
    discover_clue, elder_sign, reaction_on_event, Ability, EventPattern, EventTiming, IntExpr,
    LocationTarget, Quantity, UsageLimit, UsagePeriod,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01001";

/// Roland's two printed abilities:
///
/// - `[reaction]` "After you defeat an enemy: Discover 1 clue at your
///   location. (Limit once per round.)"
/// - `[elder_sign]` effect: "+1 for each clue on your location." (#118)
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        reaction_on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            EventTiming::After,
            discover_clue(LocationTarget::YourLocation, 1),
        )
        .with_usage_limit(UsageLimit {
            count: 1,
            period: UsagePeriod::Round,
        }),
        // [elder_sign] effect: +1 for each clue on your location. (01001.)
        elder_sign(IntExpr::Count(Quantity::CluesAtControllerLocation)),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, LocationTarget, Trigger, UsageLimit, UsagePeriod,
    };

    #[test]
    fn first_ability_is_the_reaction_with_once_per_round_limit() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Reaction,
            },
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
                count: 1,
            },
        ));
        assert_eq!(
            abilities[0].usage_limit,
            Some(UsageLimit {
                count: 1,
                period: UsagePeriod::Round,
            }),
        );
    }

    #[test]
    fn abilities_include_elder_sign_clue_count_modifier() {
        use card_dsl::dsl::{IntExpr, Quantity, Trigger};
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        // The elder-sign half: +1 for each clue on your location.
        assert_eq!(
            abilities[1].trigger,
            Trigger::ElderSign {
                modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
            },
        );
        assert!(abilities[1].usage_limit.is_none());
        // Pure-modifier elder-sign: inert empty `Seq` effect (the engine reads
        // the trigger's `modifier`, not the effect).
        assert!(matches!(&abilities[1].effect, card_dsl::dsl::Effect::Seq(v) if v.is_empty()),);
    }

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
