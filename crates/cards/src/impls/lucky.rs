//! Lucky! (Survivor event, 01080).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fortune.
//! Fast. Play when you would fail a skill test.
//! Get +2 to your skill value for that test.
//! ```
//!
//! **Cell: the `when` cell of the `SkillTestResolved` condition**, on a
//! failure. *"Play when you would fail"* names a timing point, which
//! `glossary/Fast.md` reads as a triggering condition for playing the card, and
//! the verdict ST.6 announces is provisional until that cell is done: the
//! condition's resolve step re-totals, so a +2 that turns the failure into a
//! success voids the failure's `at` and `after` cells and the test succeeds in
//! a sequence of its own (#1111). The rulings put the `when` cell first —
//! *"The "When you would fail a skill test" triggering condition comes before
//! the "After you fail a skill test" triggering condition, so you cannot play
//! Lucky! after you use Rabbit’s Foot or play "Look what I found!""*
//! (<https://arkhamdb.com/card/01080>).
//!
//! *"+2 to your skill value for that test"* is a [`Stat::TestedSkill`] row for
//! this test, folded with the other modifiers before the clamp at zero. The
//! other ruling — *"If you reveal an \[auto_fail\] auto-fail token, your total
//! skill value is considered to be 0, overriding any modifiers."* — holds
//! because the determination substitutes the total after the fold: Lucky! is
//! still playable after an automatic failure, and changes nothing.
//!
//! Two copies may both be played while the test still fails. Once the first
//! has made it succeed, *"you would fail"* no longer holds, so the second is
//! withdrawn from the window (`glossary/Triggering_Condition.md`: *"Each
//! eligible ability that triggers in reference to a specified timing point may
//! be used once each time that timing point occurs."*).
//!
//! The Fast/cost metadata comes from the generated corpus.

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, ModifierScope, Stat, TestOutcome,
    TestedLocationScope,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01080";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "Play when you would fail a skill test. / Get +2 to your skill value for
/// that test."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::SkillTestResolved {
            outcome: TestOutcome::Failure,
            kind: None,
            by_controller: true,
            tested_location: TestedLocationScope::Any,
            margin: None,
        },
        EventTiming::When,
        dsl::modify(Stat::TestedSkill, 2, ModifierScope::ThisSkillTest),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
