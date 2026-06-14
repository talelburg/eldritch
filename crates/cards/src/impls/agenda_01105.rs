//! What's Going On?! (The Gathering Agenda 1, 01105).
//!
//! ```text
//! Agenda 1 — What's Going On?! Doom: 3.
//! (reverse) The lead investigator must decide (choose one): Either each
//!   investigator discards 1 card at random from his or her hand, or the
//!   lead investigator takes 2 horror.
//! ```
//!
//! Forced on-advance reverse, fired via `ForcedTriggerPoint::AgendaAdvanced`
//! from `advance_agenda` (the mirror of the act path).
//!
//! **Deferred-choice scope (TODO #212).** The card is a *lead-investigator
//! choice* between two legal outcomes. Presenting that choice needs a
//! suspendable `AwaitingInput` mid-forced-dispatch, which the engine does
//! not have yet: `Effect::ChooseOne` is a stub (#19 shipped only the
//! test-side resolver), and the Mythos step-1.3 doom-check path
//! (`mythos_phase → check_doom_threshold → advance_agenda`) resolves
//! inline and cannot suspend. That suspendable-dispatch machinery is the
//! `emit_event` unification ([#212]) — which explicitly owns "mid-emit
//! `AwaitingInput` suspension" and will absorb `fire_forced_triggers`. A
//! bespoke suspension threaded through Mythos now would be throwaway work
//! #212 removes.
//!
//! So this ships **one of the two legal branches deterministically — the
//! lead takes 2 horror** — as a plain DSL [`deal_horror`] effect, not a
//! silent approximation: it applies an outcome the card actually offers
//! (mirroring act-3's R1/R2 choice deferred to a single latch). The horror
//! branch is chosen over the random-discard branch deliberately: "discards
//! 1 card **at random**" needs *recorded* randomness (an `EngineRecord`)
//! for replay determinism — more throwaway infra for a branch becoming
//! interactive under #212 anyway. Revisit when #212 lands.
//!
//! [#212]: https://github.com/talelburg/eldritch/issues/212

use card_dsl::dsl::{
    deal_horror, on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};

/// `ArkhamDB` code for Agenda 1, "What's Going On?!".
pub const CODE: &str = "01105";

/// 01105's Forced on-advance reverse (deferred to the deterministic
/// 2-horror-to-the-lead branch; see the module docs / TODO #212).
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![on_event(
        EventPattern::AgendaAdvanced,
        EventTiming::After,
        // `You` binds to the controller, which `AgendaAdvanced` sets to the
        // lead investigator.
        deal_horror(InvestigatorTarget::You, 2),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, InvestigatorTarget, Trigger};

    #[test]
    fn abilities_are_one_forced_on_advance_lead_two_horror() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::AgendaAdvanced,
                timing: EventTiming::After,
            }
        );
        assert_eq!(
            abilities[0].effect,
            Effect::DealHorror {
                target: InvestigatorTarget::You,
                amount: 2,
            },
            "deferred branch: the lead takes 2 horror (TODO #212 for the real choice)",
        );
    }
}
