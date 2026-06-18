//! Barricade (Seeker event, 01038).
//!
//! ```text
//! Insight. Tactic.
//! Attach to your location.
//! Non-Elite enemies cannot move into attached location.
//! Forced - When an investigator leaves attached location: Discard Barricade.
//! ```
//!
//! Three abilities on one card: `OnPlay` attaches the played event to the
//! controller's location (`Effect::AttachSelfToLocation` — one card, no
//! duplicate); a `Constant` `Restriction::EnemyMovementBlocked` (inspected by
//! hunter pathfinding — non-Elite enemies cannot path into the attached
//! location); and a `Forced` self-discard when an investigator leaves the
//! attached location (`EventPattern::LeftLocation` → `Effect::DiscardSelf`,
//! routed to the owner's player discard).

use card_dsl::dsl::{
    attach_self_to_location, constant, discard_self, forced_on_event, on_play, restrict, Ability,
    EventPattern, EventTiming, Restriction,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01038";

/// Attach-on-play, the constant non-Elite movement block, and the
/// leave-location forced self-discard.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        on_play(attach_self_to_location()),
        constant(restrict(Restriction::EnemyMovementBlocked)),
        forced_on_event(
            EventPattern::LeftLocation,
            EventTiming::After,
            discard_self(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, Restriction, Trigger, TriggerKind};

    #[test]
    fn abilities_are_attach_block_and_leave_discard() {
        let a = super::abilities();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].trigger, Trigger::OnPlay);
        assert_eq!(a[0].effect, Effect::AttachSelfToLocation);
        assert_eq!(a[1].trigger, Trigger::Constant);
        assert_eq!(
            a[1].effect,
            Effect::Restrict(Restriction::EnemyMovementBlocked)
        );
        assert!(matches!(
            a[2].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::LeftLocation,
                kind: TriggerKind::Forced,
                ..
            }
        ));
        assert_eq!(a[2].effect, Effect::DiscardSelf);
    }
}
