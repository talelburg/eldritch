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
//! move/fight/evade, read via `pending_action_surcharge` by **both** ways of
//! taking one of those actions: the basic move/fight/evade handlers, and an
//! activated ability whose bold designator names the class (#754) — a weapon's
//! *"\[action\] … **Fight**"* is a Fight action, so the first one each round
//! costs 2 exactly as punching does. The three actions share one
//! `first_each_round` budget, whichever kind spends it.
//!
//! The forced ability runs a willpower(3) `Effect::SkillTest` that
//! discards the card on **success** (`on_success = DiscardSelf`) and has no
//! failure-side effect.
//!
//! **Cell: the `at` cell of the `EndOfTurn` condition.** The printed word is
//! *"At"* — *"**Forced** - At the end of your turn: …"* — and `glossary/At.md`
//! puts those abilities *"in between any "when..." abilities and any
//! "after..." abilities with the same triggering condition."* So the turn's
//! end lands first and the willpower test runs before anything reacting to
//! the finished turn.
//!
//! # Module gap
//!
//! The surcharge reaches basic actions and action-cost abilities with a bold
//! designator, but **not fast ones**. The rulings
//! (<https://arkhamdb.com/card/01164>) go further than we implement:
//!
//! > Frozen in Fear requires 1 additional action to be spent when performing
//! > basic actions, granted actions, or Free Triggered Ability actions of the
//! > specified types.
//!
//! and, on granted movement:
//!
//! > **Follow-up Q:** To be completely clear, does Frozen in Fear make the move granted
//! > from Shortcut cost an action or not? **A:** Yes, the move on Shortcut (2)
//! > would then cost an action.
//!
//! Neither is reachable in the current corpus — every implemented ability with
//! a Move/Fight/Evade designator prints an action cost, and no granted-action
//! effect exists — and taxing a fast ability needs a decision about an actor
//! spending actions outside their own turn. Tracked as #759.

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    constant, discard_self, forced_on_event, put_into_threat_area, restrict, revelation,
    skill_test, Ability, ActionClass, EventPattern, EventTiming, Restriction,
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
        forced_on_event(
            EventPattern::EndOfTurn,
            EventTiming::At,
            // Test willpower(3): on success discard Frozen in Fear.
            skill_test(SkillKind::Willpower, 3, Some(discard_self()), None),
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
        assert!(
            matches!(&abilities[0].effect, Effect::PutIntoThreatArea { code, clues: 0 } if code == CODE)
        );

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
                timing: EventTiming::At,
                ..
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
