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
//! `resolve_encounter_card` does not auto-discard it. The Revelation
//! native places it in the controller's threat area. The surcharge is
//! `Restriction::ExtraActionCost { first_each_round: true }` over
//! move/fight/evade, read by those handlers via `pending_action_surcharge`.
//! The forced ability runs a willpower(3) [`Effect::SkillTest`] that
//! discards the card on **success** (`on_success = DiscardSelf`) and does
//! nothing on failure (`Effect::Seq(vec![])`).

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    constant, discard_self, native, on_event, restrict, revelation, skill_test_with_success,
    Ability, ActionClassSet, Effect, EventPattern, EventTiming, Restriction,
};
use game_core::card_registry::NativeEffectFn;
use game_core::state::CardCode;
use game_core::{place_in_threat_area, Cx, EngineOutcome, EvalContext};

/// `ArkhamDB` code for Frozen in Fear.
pub const CODE: &str = "01164";

const TO_THREAT_AREA: &str = "01164:to-threat-area";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        revelation(native(TO_THREAT_AREA)),
        constant(restrict(Restriction::ExtraActionCost {
            actions: ActionClassSet {
                move_: true,
                fight: true,
                evade: true,
            },
            first_each_round: true,
        })),
        on_event(
            EventPattern::EndOfTurn,
            EventTiming::After,
            // Test willpower(3): on success discard Frozen in Fear; on
            // failure do nothing.
            skill_test_with_success(SkillKind::Willpower, 3, discard_self(), Effect::Seq(vec![])),
        ),
    ]
}

/// Resolve this treachery's native-effect tag. Wired into the crate
/// registry's `native_effect_for`.
pub(crate) fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    (tag == TO_THREAT_AREA).then_some(to_threat_area as NativeEffectFn)
}

/// Revelation: put Frozen in Fear into the controller's threat area.
fn to_threat_area(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    place_in_threat_area(cx, ctx.controller, CardCode::new(CODE));
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::Trigger;

    #[test]
    fn abilities_are_threat_area_surcharge_and_end_of_turn_test() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 3);

        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert!(matches!(&abilities[0].effect, Effect::Native { tag } if tag == TO_THREAT_AREA));

        assert_eq!(abilities[1].trigger, Trigger::Constant);
        assert!(matches!(
            &abilities[1].effect,
            Effect::Restrict(Restriction::ExtraActionCost {
                actions: ActionClassSet {
                    move_: true,
                    fight: true,
                    evade: true
                },
                first_each_round: true,
            })
        ));

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
        assert!(matches!(**on_fail, Effect::Seq(ref v) if v.is_empty()));

        assert!(native_effect_for(TO_THREAT_AREA).is_some());
        assert!(native_effect_for("nope").is_none());
    }
}
