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
//! `resolve_encounter_card` does not auto-discard it. The Revelation
//! native places it in the controller's threat area; the `CannotPlay`
//! restrictions are read by `play_card` via `play_is_prohibited`; the
//! forced `Effect::DiscardSelf` fires on `RoundEnded` (resolving
//! alongside the agenda's round-end forced effect via the dispatcher's
//! deterministic multi-resolve).

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    constant, discard_self, native, on_event, restrict, revelation, Ability, EventPattern,
    EventTiming, Restriction,
};
use game_core::card_registry::NativeEffectFn;
use game_core::state::CardCode;
use game_core::{place_in_threat_area, Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for Dissonant Voices.
pub const CODE: &str = "01165";

const TO_THREAT_AREA: &str = "01165:to-threat-area";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(native(TO_THREAT_AREA)),
        constant(restrict(Restriction::CannotPlay(CardType::Asset))),
        constant(restrict(Restriction::CannotPlay(CardType::Event))),
        on_event(EventPattern::RoundEnded, EventTiming::After, discard_self()),
    ]
}

/// Resolve this treachery's native-effect tag. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == TO_THREAT_AREA).then_some(to_threat_area as NativeEffectFn)
}

/// Revelation: put Dissonant Voices into the controller's threat area.
fn to_threat_area(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    place_in_threat_area(cx, ctx.controller, CardCode::new(CODE));
    EngineOutcome::Done
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
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == TO_THREAT_AREA));

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
            }
        ));
        assert!(matches!(&abilities[3].effect, Effect::DiscardSelf));

        assert!(native_effect_for(TO_THREAT_AREA).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
