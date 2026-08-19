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
//! Act-3's *reverse* (the R1/R2 resolution choice) is deferred to Phase 9
//! (campaign log gives the branch meaning); the scenario keeps a single
//! Won/R1 latch. The Ghoul Priest enemy + its spawn land in C3 (#231);
//! this objective is unit-tested here and proven end-to-end in C7b (#245).

use card_dsl::dsl::{advance_current_act, forced_on_event, Ability, EventPattern, EventTiming};

/// `ArkhamDB` code for Act 3, "What Have You Done?".
pub const CODE: &str = "01110";

/// 01110's Forced objective: advance when the Ghoul Priest is defeated.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::EnemyDefeated {
            by_controller: false,
            code: Some("01116".to_owned()),
        },
        EventTiming::At,
        advance_current_act(),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger};

    #[test]
    fn abilities_advance_on_ghoul_priest_defeat() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
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
}
