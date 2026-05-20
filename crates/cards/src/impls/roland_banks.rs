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
//! `abilities()` ships only the `[reaction]` half. The `[elder_sign]`
//! half stays as the engine-wide `+0` placeholder until the dynamic
//! skill-test modifier DSL primitive lands; tracked in
//! [issue #118](https://github.com/talelburg/eldritch/issues/118).
//!
//! The reaction compiles to a [`Trigger::OnEvent`] with the
//! [`EventPattern::EnemyDefeated`] pattern narrowed by
//! `by_controller: true` (Roland's "After **you** defeat an enemy")
//! at [`EventTiming::After`], discovering 1 clue at
//! [`LocationTarget::ControllerLocation`]. The "Limit once per round"
//! clause attaches as a [`UsageLimit`] with `count: 1, period:
//! UsagePeriod::Round`.
//!
//! [`Trigger::OnEvent`]: card_dsl::dsl::Trigger::OnEvent
//! [`EventPattern::EnemyDefeated`]: card_dsl::dsl::EventPattern::EnemyDefeated
//! [`EventTiming::After`]: card_dsl::dsl::EventTiming::After
//! [`LocationTarget::ControllerLocation`]: card_dsl::dsl::LocationTarget::ControllerLocation
//! [`UsageLimit`]: card_dsl::dsl::UsageLimit

use card_dsl::dsl::{
    discover_clue, on_event, Ability, EventPattern, EventTiming, LocationTarget, UsageLimit,
    UsagePeriod,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01001";

/// Roland's `[reaction]` "After you defeat an enemy: Discover 1 clue
/// at your location. (Limit once per round.)" The `[elder_sign]` half
/// is tracked separately (#118).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnemyDefeated {
            by_controller: true,
        },
        EventTiming::After,
        discover_clue(LocationTarget::ControllerLocation, 1),
    )
    .with_usage_limit(UsageLimit {
        count: 1,
        period: UsagePeriod::Round,
    })]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, LocationTarget, Trigger, UsageLimit, UsagePeriod,
    };

    #[test]
    fn abilities_are_one_reaction_with_once_per_round_limit() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                },
                timing: EventTiming::After,
            },
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::ControllerLocation,
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

    /// Catches a `pub mod` rename or a fat-fingered match arm in
    /// `impls::abilities_for` — the registry must dispatch CODE to
    /// this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
