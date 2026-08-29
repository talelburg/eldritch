//! What Have You Done? (The Gathering Act 3, 01110).
//!
//! ```text
//! Act 3 — What Have You Done?
//! Objective – If the Ghoul Priest is Defeated, advance.
//! ```
//!
//! Forced (no "may" — Rules Reference p.3; the bare "advance" with no
//! clue threshold cannot be the optional clue-spend ability): the act
//! advances when the Ghoul Priest (01116) is defeated, firing its terminal
//! Won/R1 resolution. Wired via `ForcedTriggerPoint::EnemyDefeated` from the
//! defeat path; narrowed to 01116 so other ghouls' defeats don't advance it.
//!
//! **Cell: the `at` cell of the `EnemyDefeated` condition.** The printed word
//! is *"If"* on a state that has already settled — and `glossary/If.md`
//! reaches this card by name: *"Some abilities have triggering conditions
//! that use the words "at" or "if" instead of specifying "when" or "after,"
//! such as "at the end of the round," or "if the Ghoul Priest is defeated."
//! These abilities trigger in between any "when..." abilities and any
//! "after..." abilities with the same triggering condition."* The ruling
//! agrees in card terms: *"The **Objective** ability is mandatory, it will
//! trigger as soon as you defeat the Ghoul Priest, before any "After you
//! defeat an enemy" reactions can be used."*
//! (<https://arkhamdb.com/card/01110>). So the defeat's own impact — the
//! enemy leaving play, its 2 victory points reaching the victory display —
//! lands first, and the advance still beats every `after` reaction to it.
//!
//! **The reverse**, `back_text` verbatim
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//!
//! > The lead investigator must decide (choose one):
//! > - It was never much of a home. Burn it down! **(→R1)**
//! > - This hell-pit is my home! No way are we burning it! **(→R2)**
//!
//! It reaches its resolution point by *running an effect* — `reach_resolution`
//! (ADR 0013). This is 01110 being terminal in the ordinary way: it is the last
//! card in the act deck, so advancing it flips it and its reverse ends the
//! scenario.
//!
//! **Cell: the `after` cell of the `ActAdvanced` condition**, for the reverse.
//! The reverse prints no trigger word, because it is not a triggered ability: it
//! is step 2 of the advance procedure — *"Flip the advancing card over and
//! follow the instructions on the reverse ("b") side."*
//! (`glossary/Act_Deck_and_Agenda_Deck.md`). Declaring it in the `after` cell of
//! the flip is what puts it where the procedure puts it: after step 1's token
//! removal and the flip itself, and before step 3's *"the next card in the deck
//! becomes the current act/agenda"* — which for a terminal act never comes,
//! since the `AdvanceReverse` frame holds the cursor and there is no next card.
//! The same cell 01108 and 01109 declare. The card's rulings
//! (<https://arkhamdb.com/card/01110>) reach only the Objective's timing, quoted
//! above; nothing contests this cell.
//!
//! The Ghoul Priest enemy + its spawn land in C3 (#231);
//! this objective is unit-tested here and proven end-to-end in C7b (#245).
//!
//! # Module gap
//!
//! **The reverse's choice is not modelled; this module reaches R1
//! unconditionally (#775).** The card prints two resolution points and asks the
//! lead investigator to pick between them, so R2 is unreachable and the lead is
//! never asked. Unchanged from what shipped when the point was a `resolution`
//! field on the act (ADR 0012), and deliberately so: #808 is the mechanism,
//! and **#775** is the choice — an `Effect::ChooseOne` of the two printed
//! options, anchored to `OptionTarget::Act`. Its consequences (trauma, the
//! campaign log, earning the Lita Chantler card) stay with #766.

use card_dsl::dsl::{
    advance_current_act, forced_on_event, reach_resolution, Ability, EventPattern, EventTiming,
};

/// `ArkhamDB` code for Act 3, "What Have You Done?".
pub const CODE: &str = "01110";

/// 01110's Forced objective (advance when the Ghoul Priest is defeated) and its
/// on-advance reverse (reach Resolution 1 — see the module doc on #775).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        forced_on_event(
            EventPattern::EnemyDefeated {
                by_controller: false,
                code: Some("01116".to_owned()),
            },
            EventTiming::At,
            advance_current_act(),
        ),
        forced_on_event(
            EventPattern::ActAdvanced,
            EventTiming::After,
            reach_resolution(1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_advance_on_ghoul_priest_defeat() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: false,
                    code: Some("01116".into()),
                },
                timing: EventTiming::At,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(abilities[0].effect, Effect::AdvanceCurrentAct));
    }

    /// The reverse reaches R1 by *running an effect* on the `after` cell of the
    /// act's own advance (ADR 0013). Unconditional: the printed `ChooseOne` of
    /// R1/R2 is #775, and until it lands this is behaviour-identical to the
    /// deleted `Act.resolution` field.
    #[test]
    fn reverse_reaches_resolution_one_after_the_act_advances() {
        let abilities = super::abilities();
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(
            matches!(abilities[1].effect, Effect::ReachResolution(1)),
            "the reverse reaches R1, got {:?}",
            abilities[1].effect
        );
    }
}
