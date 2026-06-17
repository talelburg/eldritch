//! Cellar (The Gathering location, 01114).
//!
//! ```text
//! Shroud: 4. Clues: 2. Victory 1.
//! Forced - After you enter the Cellar: Take 1 damage.
//! ```
//!
//! Forced-on-enter via the `EnteredLocation` dispatch path; the
//! controller binding is the entering investigator ("you"). Shroud /
//! Clues / Victory are location state set by `setup()`.

use card_dsl::dsl::{
    deal_damage, forced_on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};

/// `ArkhamDB` code for the Cellar.
pub const CODE: &str = "01114";

/// The Cellar's Forced "after you enter: take 1 damage".
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::EnteredLocation,
        EventTiming::After,
        deal_damage(InvestigatorTarget::You, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, HarmKind, InvestigatorTarget, Trigger};

    #[test]
    fn abilities_are_one_forced_enter_damage() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredLocation,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::Deal {
                kind: HarmKind::Damage,
                target: InvestigatorTarget::You,
                amount: 1,
            }
        ));
    }
}
