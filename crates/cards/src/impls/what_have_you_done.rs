//! What Have You Done? (The Gathering Act 3, 01110).
//!
//! ```text
//! Act 3 — What Have You Done?
//! Objective – If the Ghoul Priest is Defeated, advance.
//! ```
//!
//! Forced (no "may" — Rules Reference p.3; the bare "advance" with no
//! clue threshold cannot be the optional clue-spend ability): the act
//! advances when the Ghoul Priest (01116) is defeated, flipping it to the
//! terminal reverse below. Wired via `ForcedTriggerPoint::EnemyDefeated` from the
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
//! scenario. **Which** point it reaches is the lead investigator's to decide,
//! so the reverse is an `Effect::ChooseOne` of the two printed bullets. Each is
//! copied verbatim as its branch's label, `(→R1)` / `(→R2)` markers included —
//! a player choosing an ending should see which ending it is — and the two
//! branches are `reach_resolution(1)` and `reach_resolution(2)` (#775).
//!
//! The controller of a Forced ability declared on the act is
//! `turn_order.first()`, which stands in for the printed *"lead investigator"*;
//! the two coincide in solo. See **Lead investigator** in `CONTEXT.md` for
//! where the proxy diverges in multiplayer.
//!
//! The options anchor to `OptionTarget::Act`, so the host renders them on the
//! act card — whose reverse face is already what the advance shows at
//! `FireReverse` (#558) — rather than in the prompt banner (#555).
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
//! **What each ending *means* is not modelled.** Both branches end the scenario
//! at their printed point and nothing else: the trauma, the campaign log, and
//! the lead investigator earning the Lita Chantler card at R1 are campaign
//! machinery and stay with #766, which is why `apply_resolution` is still a
//! stub. The board tells the two apart today — ADR 0012 put the win/loss
//! projection at the display boundary, so the banner reads *"Scenario ended —
//! Resolution 1"* or *"— Resolution 2"* off `GameState.ending`.

use card_dsl::dsl::{
    advance_current_act, choose_one, forced_on_event, reach_resolution, Ability, EventPattern,
    EventTiming,
};

/// `ArkhamDB` code for Act 3, "What Have You Done?".
pub const CODE: &str = "01110";

/// The first printed bullet of 01110's reverse, verbatim.
const BURN_IT_DOWN_LABEL: &str = "It was never much of a home. Burn it down! (→R1)";
/// The second printed bullet, verbatim.
const MY_HOME_LABEL: &str = "This hell-pit is my home! No way are we burning it! (→R2)";

/// 01110's Forced objective (advance when the Ghoul Priest is defeated) and its
/// on-advance reverse (the lead investigator's choice of resolution point).
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
            // The bullets are printed as a list, so each label is one of them
            // copied verbatim rather than derived.
            choose_one([
                (BURN_IT_DOWN_LABEL, reach_resolution(1)),
                (MY_HOME_LABEL, reach_resolution(2)),
            ]),
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

    /// The reverse offers the lead investigator the two printed resolution
    /// points on the `after` cell of the act's own advance (ADR 0013). Both
    /// bullets are reachable; the labels are the printed text, markers and all.
    #[test]
    fn reverse_offers_both_printed_resolution_points_after_the_act_advances() {
        let abilities = super::abilities();
        assert_eq!(
            abilities[1].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::ActAdvanced,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        let Effect::ChooseOne(branches) = &abilities[1].effect else {
            panic!(
                "expected the R1/R2 ChooseOne, got {:?}",
                abilities[1].effect
            );
        };
        assert_eq!(branches.len(), 2);
        assert_eq!(
            branches[0].label,
            "It was never much of a home. Burn it down! (→R1)"
        );
        assert!(matches!(branches[0].effect, Effect::ReachResolution(1)));
        assert_eq!(
            branches[1].label,
            "This hell-pit is my home! No way are we burning it! (→R2)"
        );
        assert!(matches!(branches[1].effect, Effect::ReachResolution(2)));
    }
}
