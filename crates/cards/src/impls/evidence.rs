//! Evidence! (Neutral event, 01022).
//!
//! Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! ```text
//! Fast. Play after you defeat an enemy.
//! Discover 1 clue at your location.
//! ```
//!
//! # Scope
//!
//! Evidence! is Roland Banks 01001's `[reaction]` ("After you defeat an
//! enemy: Discover 1 clue at your location.") sourced from hand instead of
//! from play — the identical [`Trigger::OnEvent`] declaration, minus Roland's
//! once-per-round [`UsageLimit`]. Per Rules Reference p.11, a Fast event with
//! a "Play after …" instruction plays "as if the described timing point were a
//! triggering condition", so the play-timing predicate IS the `OnEvent`
//! pattern (Axis C, #335 / #304). The Fast/cost/Insight metadata comes from
//! the generated corpus.
//!
//! **Cell: the `after` cell of the `EnemyDefeated` condition.** The printed
//! word is *"after"* — *"Fast. Play after you defeat an enemy."* — and
//! `glossary/After.md` puts that *"immediately after the specified timing point
//! or triggering condition has fully resolved."* So the defeat is complete —
//! the enemy out of play, any victory points banked — before the clue is
//! discovered, which is also what puts this behind act 01110's `at`-cell *"If
//! the Ghoul Priest is Defeated"* objective on the same defeat. The ruling
//! constrains the effect rather than the cell: *"You can only 'discover' a clue
//! if there is a clue on your location."* (<https://arkhamdb.com/card/01022>).
//!
//! [`Trigger::OnEvent`]: card_dsl::dsl::Trigger::OnEvent
//! [`UsageLimit`]: card_dsl::dsl::UsageLimit

use card_dsl::dsl::{
    discover_clue, reaction_on_event, Ability, EventPattern, EventTiming, LocationTarget,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01022";

/// Evidence!'s "Play after you defeat an enemy. / Discover 1 clue at your
/// location." — Roland 01001's reaction without the usage limit.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        EventTiming::After,
        discover_clue(LocationTarget::YourLocation, 1),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, EventPattern, EventTiming, LocationTarget, Trigger, TriggerKind};

    #[test]
    fn abilities_are_one_after_defeat_reaction_discovering_one_clue() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(
            abilities[0].effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
                count: 1,
            },
        ));
        assert!(
            abilities[0].usage_limit.is_none(),
            "Evidence! is a one-shot event — no per-round usage limit",
        );
    }

    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
