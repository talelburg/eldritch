//! Emergency Cache (Neutral event, 01088).
//!
//! ```text
//! Gain 3 resources.
//! ```
//!
//! A plain `OnPlay` resource gain — the simplest player event.

use card_dsl::dsl::{gain_resources, on_play, Ability, InvestigatorTarget};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01088";

/// On play, gain 3 resources.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_play(gain_resources(InvestigatorTarget::You, 3))]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, InvestigatorTarget, Trigger};

    #[test]
    fn abilities_are_one_on_play_gain_three_resources() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::OnPlay);
        assert_eq!(
            abilities[0].effect,
            Effect::GainResources {
                target: InvestigatorTarget::You,
                amount: 3,
            }
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
