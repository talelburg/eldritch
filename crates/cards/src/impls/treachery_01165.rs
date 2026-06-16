//! Dissonant Voices (The Gathering treachery, 01165).
//!
//! ```text
//! Revelation - Put Dissonant Voices into play in your threat area.
//! You cannot play assets or events.
//! Forced - At the end of the round: Discard Dissonant Voices.
//! ```
//!
//! Persistent treachery: it has non-Revelation abilities (two constant
//! `CannotPlay` restrictions and a forced self-discard), so
//! `resolve_encounter_card` does not auto-discard it. The Revelation uses
//! the shared `Effect::PutIntoThreatArea`; the `CannotPlay` restrictions
//! are read by `play_card` via `play_is_prohibited`; the forced
//! `Effect::DiscardSelf` fires on `RoundEnded` (resolving alongside the
//! agenda's round-end forced effect via the dispatcher's deterministic
//! multi-resolve).

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    constant, discard_self, forced_on_event, put_into_threat_area, restrict, revelation, Ability,
    EventPattern, EventTiming, Restriction,
};

/// `ArkhamDB` code for Dissonant Voices.
pub const CODE: &str = "01165";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area(CODE)),
        constant(restrict(Restriction::CannotPlay(CardType::Asset))),
        constant(restrict(Restriction::CannotPlay(CardType::Event))),
        forced_on_event(EventPattern::RoundEnded, EventTiming::After, discard_self()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn abilities_are_threat_area_two_play_bans_and_forced_discard() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 4);

        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(
            matches!(&abilities[0].effect, Effect::PutIntoThreatArea { code, clues: 0 } if code == CODE)
        );

        assert_eq!(abilities[1].trigger, Trigger::Constant);
        assert!(matches!(
            &abilities[1].effect,
            Effect::Restrict(Restriction::CannotPlay(CardType::Asset))
        ));
        assert!(matches!(
            &abilities[2].effect,
            Effect::Restrict(Restriction::CannotPlay(CardType::Event))
        ));

        assert!(matches!(
            &abilities[3].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::RoundEnded,
                timing: EventTiming::After,
                ..
            }
        ));
        assert!(matches!(&abilities[3].effect, Effect::DiscardSelf));
    }
}
