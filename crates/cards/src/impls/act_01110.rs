//! What Have You Done? (The Gathering Act 3, 01110).
//!
//! ```text
//! Act 3 — What Have You Done?
//! Objective – If the Ghoul Priest is Defeated, advance.
//! ```
//!
//! Forced (no "may" — Rules Reference p.3; the bare "advance" with no
//! clue threshold cannot be the optional clue-spend ability): the act
//! advances the instant the Ghoul Priest (01116) is defeated, firing its
//! terminal Won/R1 resolution. Wired via `ForcedTriggerPoint::EnemyDefeated`
//! from the defeat path; narrowed to 01116 so other ghouls' defeats don't
//! advance it.
//!
//! Act-3's *reverse* (the R1/R2 resolution choice) is deferred to Phase 9
//! (campaign log gives the branch meaning); the scenario keeps a single
//! Won/R1 latch. The Ghoul Priest enemy + its spawn land in C3 (#231);
//! this objective is unit-tested here and proven end-to-end in C7b (#245).

use card_dsl::dsl::{advance_current_act, on_event, Ability, EventPattern, EventTiming};

/// `ArkhamDB` code for Act 3, "What Have You Done?".
pub const CODE: &str = "01110";

/// 01110's Forced objective: advance when the Ghoul Priest is defeated.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::EnemyDefeated {
            by_controller: false,
            code: Some("01116".to_owned()),
        },
        EventTiming::After,
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
                timing: EventTiming::After,
            }
        );
        assert!(matches!(abilities[0].effect, Effect::AdvanceCurrentAct));
    }
}
