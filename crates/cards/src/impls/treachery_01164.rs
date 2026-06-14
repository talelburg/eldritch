//! Frozen in Fear (The Gathering treachery, 01164).
//!
//! ```text
//! Revelation - Put Frozen in Fear into play in your threat area.
//! The first time you perform one of the following actions (move, fight,
//!   or evade) each round, it costs 1 additional action.
//! Forced - At the end of your turn: Test [willpower] (3). If you succeed,
//!   discard Frozen in Fear.
//! ```
//!
//! Persistent treachery: it has non-Revelation abilities (a constant
//! action surcharge and a forced end-of-turn test), so
//! `resolve_encounter_card` does not auto-discard it. The Revelation uses
//! the shared `Effect::PutIntoThreatArea`. The surcharge is
//! `Restriction::ExtraActionCost { first_each_round: true }` over
//! move/fight/evade, read by those handlers via `pending_action_surcharge`.
//! The forced ability runs a willpower(3) `Effect::SkillTest` that
//! discards the card on **success** (`on_success = DiscardSelf`) and has no
//! failure-side effect.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    constant, discard_self, on_event, put_into_threat_area, restrict, revelation,
    skill_test_with_success, Ability, ActionClass, EventPattern, EventTiming, Restriction,
};

/// `ArkhamDB` code for Frozen in Fear.
pub const CODE: &str = "01164";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(put_into_threat_area(CODE)),
        constant(restrict(Restriction::ExtraActionCost {
            actions: vec![ActionClass::Move, ActionClass::Fight, ActionClass::Evade],
            first_each_round: true,
        })),
        on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            // Test willpower(3): on success discard Frozen in Fear.
            skill_test_with_success(SkillKind::Willpower, 3, discard_self()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, Trigger};

    #[test]
    fn abilities_are_threat_area_surcharge_and_end_of_turn_test() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);

        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(&abilities[0].effect, Effect::PutIntoThreatArea { code } if code == CODE));

        assert_eq!(abilities[1].trigger, Trigger::Constant);
        let Effect::Restrict(Restriction::ExtraActionCost {
            actions,
            first_each_round,
        }) = &abilities[1].effect
        else {
            panic!("expected ExtraActionCost, got {:?}", abilities[1].effect);
        };
        assert_eq!(
            actions,
            &[ActionClass::Move, ActionClass::Fight, ActionClass::Evade]
        );
        assert!(first_each_round);

        assert!(matches!(
            &abilities[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EndOfTurn,
                timing: EventTiming::After,
            }
        ));
        let Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } = &abilities[2].effect
        else {
            panic!("expected SkillTest, got {:?}", abilities[2].effect);
        };
        assert_eq!(*skill, SkillKind::Willpower);
        assert_eq!(*difficulty, 3);
        assert!(matches!(on_success.as_deref(), Some(Effect::DiscardSelf)));
        assert!(on_fail.is_none(), "no failure-side effect");
    }
}
