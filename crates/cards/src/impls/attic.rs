//! Attic (The Gathering location, 01113).
//!
//! ```text
//! Shroud: 1. Clues: 2. Victory 1.
//! Forced - After you enter the Attic: Take 1 horror.
//! ```
//!
//! Forced-on-enter via the `EnteredLocation` dispatch path
//! (`engine::dispatch::forced_triggers`); the controller binding is the
//! entering investigator ("you"). The Victory 1 and Clues 2 are location
//! *state* set by the scenario's `setup()`, not ability data — only the
//! Forced horror lives here.
//!
//! **Cell: the `after` cell of the `EnteredLocation` condition.** The printed
//! word is *"After"* — *"**Forced** - After you enter the Attic: Take 1
//! horror."* — and `glossary/After.md` puts that *"immediately after the
//! specified timing point or triggering condition has fully resolved."* So the
//! entry is finished, with the investigator already at the Attic, before the
//! horror lands. The ruling scopes *whose* entry counts, not when it lands:
//! *"The **Forced** ability triggers each time an investigator enters this
//! location."* (<https://arkhamdb.com/card/01113>).

use card_dsl::dsl::{
    deal_horror, forced_on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};

/// `ArkhamDB` code for the Attic.
pub const CODE: &str = "01113";

/// The Attic's Forced "after you enter: take 1 horror".
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![forced_on_event(
        EventPattern::EnteredLocation,
        EventTiming::After,
        deal_horror(InvestigatorTarget::You, 1u8),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        Effect, EventPattern, EventTiming, HarmKind, IntExpr, InvestigatorTarget, Trigger,
    };

    #[test]
    fn abilities_are_one_forced_enter_horror() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredLocation,
                timing: EventTiming::After,
                kind: card_dsl::dsl::TriggerKind::Forced,
            }
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::Deal {
                kind: HarmKind::Horror,
                target: InvestigatorTarget::You,
                amount: IntExpr::Lit(1),
            }
        ));
    }
}
