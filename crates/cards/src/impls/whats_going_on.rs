//! What's Going On?! (The Gathering Agenda 1, 01105).
//!
//! ```text
//! Agenda 1 — What's Going On?! Doom: 3.
//! (reverse) The lead investigator must decide (choose one): Either each
//!   investigator discards 1 card at random from his or her hand, or the
//!   lead investigator takes 2 horror.
//! ```
//!
//! Forced on-advance reverse, fired at the agenda's own `AgendaAdvanced` condition
//! from `advance_agenda` (the mirror of the act path).
//!
//! **Interactive choice (Axis A, #334).** The lead-investigator "choose one"
//! is an `Effect::ChooseOne` over the two printed branches, each carrying the
//! label the lead reads. **Those labels split one printed sentence** — 01105
//! prints its options in prose, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > The lead investigator must decide (choose one): Either each investigator
//! > discards 1 card at random from his or her hand, or the lead investigator
//! > takes 2 horror.
//!
//! — so labelling means dividing it at its own *"Either …, or …"*, into *"Each
//! investigator discards 1 card at random from his or her hand"* and *"The lead
//! investigator takes 2 horror"*. Nothing is added. The choice
//! suspends inside the Forced run (auto-resolving only if one branch is the
//! sole legal option — both always are here, so it round-trips), and the
//! Axis-B reentrant forced loop resumes after the lead picks.
//!
//! The random-discard branch is an `Effect::ForEach` over every investigator,
//! in turn order, whose body is a card-local `Effect::Native` discarding one
//! card at random from the investigator it is running for, via
//! [`NativeCx::discard_random_from_hand`]. The order shows only in which
//! investigator's discard draws from the RNG first. The randomness replays
//! deterministically from the engine's `(seed, draws)` RNG (no `EngineRecord`
//! is needed — see that helper's docs); the earlier "needs recorded
//! randomness" deferral note was incorrect.
//!
//! **Cell: the `after` cell of the `AgendaAdvanced` condition.** The reverse
//! prints no trigger word, because it is not a triggered ability: it is step 2
//! of the Rules Reference's advance procedure — *"Flip the advancing card over
//! and follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — the `AdvanceReverse` frame holds the deck
//! cursor until its `Finalize` step, which it reaches only once this choice has
//! drained. With no printed word to read against, nothing contests the cell.
//! The ruling is about the choice, not the timing: *"The lead investigator can
//! choose the first option even if one of the investigators has no cards in
//! hand (but at least one does)."* (<https://arkhamdb.com/card/01105>).

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, InvestigatorTarget, InvestigatorTargetSet,
};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Agenda 1, "What's Going On?!".
pub const CODE: &str = "01105";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(RANDOM_DISCARD, random_discard)]);

const RANDOM_DISCARD: &str = "01105:random-discard";

/// Label for the first half of the printed *"Either …, or …"* sentence.
const DISCARD_EACH_LABEL: &str = "Each investigator discards 1 card at random from his or her hand";
/// Label for its second half.
const LEAD_TAKES_HORROR_LABEL: &str = "The lead investigator takes 2 horror";

/// 01105's Forced on-advance reverse: the lead's "choose one" between each
/// investigator discarding 1 random card or the lead taking 2 horror.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::AgendaAdvanced,
        EventTiming::After,
        // The labels split the one printed sentence quoted in the module doc:
        // its "Either …, or …" is the division, so each branch's label is one
        // of the two halves.
        dsl::choose_one(vec![
            // Branch A: each investigator discards 1 card at random.
            (
                DISCARD_EACH_LABEL,
                dsl::for_each(InvestigatorTargetSet::All, dsl::native(RANDOM_DISCARD)),
            ),
            // Branch B: the lead (`You`, bound to the lead by `AgendaAdvanced`)
            // takes 2 horror.
            (
                LEAD_TAKES_HORROR_LABEL,
                dsl::deal_horror(InvestigatorTarget::You, 2u8),
            ),
        ]),
    )]
}

/// Branch A's body: the investigator the `ForEach` is running for discards 1
/// card at random from hand. A single-consumer native in the Crypt Chill mold.
fn random_discard(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(investigator) = ctx.each_investigator() else {
        return EngineOutcome::Rejected {
            reason: "01105 random-discard: run outside its ForEach over investigators".into(),
        };
    };
    // Empty hand ⇒ no-op (helper returns None); not an error.
    let _ = cx.discard_random_from_hand(investigator);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, ForEachTargets, HarmKind, IntExpr, InvestigatorTarget,
        InvestigatorTargetSet, Trigger, TriggerKind,
    };

    #[test]
    fn abilities_are_one_forced_on_advance_choose_one() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::AgendaAdvanced,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            }
        );
        let Effect::ChooseOne(branches) = &abilities[0].effect else {
            panic!("expected ChooseOne, got {:?}", abilities[0].effect);
        };
        assert_eq!(branches.len(), 2, "random-discard-each vs. lead 2 horror");
        let Effect::ForEach { targets, body } = &branches[0].effect else {
            panic!("branch A fans out, got {:?}", branches[0].effect);
        };
        assert_eq!(
            *targets,
            ForEachTargets::Investigators(InvestigatorTargetSet::All),
            "branch A walks every investigator",
        );
        assert!(
            matches!(&**body, Effect::Native { tag } if tag == super::RANDOM_DISCARD),
            "branch A's body is the random-discard native",
        );
        assert!(
            matches!(
                &branches[1].effect,
                Effect::Deal {
                    kind: HarmKind::Horror,
                    target: InvestigatorTarget::You,
                    amount: IntExpr::Lit(2),
                    direct: false,
                }
            ),
            "branch B is the lead taking 2 horror",
        );
        // The labels are what the lead reads; before #775 this rendered the
        // branches' `Debug` form, tag and all. Asserted as literals — a const
        // checked against itself would pass through a typo in the split.
        assert_eq!(
            branches[0].label,
            "Each investigator discards 1 card at random from his or her hand"
        );
        assert_eq!(branches[1].label, "The lead investigator takes 2 horror");
    }
}
