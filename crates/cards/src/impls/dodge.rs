//! Dodge (Neutral Tactic event, 01023).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fast. Play when an enemy attacks an investigator at your location.
//! Cancel that attack.
//! ```
//!
//! A reaction event played from hand in the `BeforeEnemyAttack` window
//! (Axis C + Axis D). Per Rules Reference p.11 a Fast event with a "Play
//! when …" instruction plays "as if the described timing point were a
//! triggering condition", so the play-timing predicate IS the
//! [`Trigger::OnEvent`] pattern (the same machinery Evidence! 01022 uses).
//! "Cancel that attack" is [`Effect::Cancel`]: the emit site (the
//! enemy-attack loop) skips the attack's damage/horror but still exhausts the
//! attacker (RR p.6 + p.25). The Fast/cost metadata comes from the corpus.
//!
//! [`Trigger::OnEvent`]: card_dsl::dsl::Trigger::OnEvent
//! [`Effect::Cancel`]: card_dsl::dsl::Effect::Cancel

use card_dsl::dsl::{reaction_on_event, Ability, Effect, EventPattern, EventTiming};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01023";

/// Dodge's "Play when an enemy attacks an investigator at your location. /
/// Cancel that attack." — a Before-timing reaction that cancels the attack.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyAttacks,
        EventTiming::Before,
        Effect::Cancel,
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn one_before_enemy_attack_reaction_that_cancels() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyAttacks,
                timing: EventTiming::Before,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(abilities[0].effect, Effect::Cancel));
        assert!(
            abilities[0].usage_limit.is_none(),
            "Dodge is a one-shot event — no per-round usage limit",
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
