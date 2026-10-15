//! Agnes Baker (Mystic investigator, 01004).
//!
//! ```text
//! Agnes Baker. The Waitress.
//! Sorcerer.
//! Willpower 5. Intellect 2. Combat 2. Agility 3.
//! Health 6. Sanity 8.
//!
//! [reaction] After 1 or more horror is placed on Agnes Baker: Deal 1
//! damage to an enemy at your location. (Limit once per phase.)
//! [elder_sign] effect: +1 for each horror on Agnes Baker.
//! ```
//!
//! # Scope
//!
//! Both printed abilities, in Roland Banks 01001's registration shape.
//!
//! The reaction is a [`Trigger::OnEvent`] on [`EventPattern::HarmPlaced`]
//! `{ Horror, YourInvestigator }` at [`EventTiming::After`]: *placed on* her
//! investigator card, which reads only the investigator card's share of the
//! placement. Her ruling (<https://arkhamdb.com/card/01004>): *"You cannot use
//! Agnes' ability if you assign horror to an asset."* — so horror soaked by an
//! asset (Holy Rosary 01059) does not count, while harm taken as a cost
//! (Forbidden Knowledge 01058's *"take 1 horror"*) is dealt and placed like any
//! other and does. The effect deals 1 damage to an enemy chosen at her
//! location; with none there the change-state check proves it inert, so it is
//! not offered. *"(Limit once per phase.)"* is a [`UsageLimit`] with
//! [`UsagePeriod::Phase`].
//!
//! **Cell: the `after` cell of the `DamagePlaced` condition.** The printed word
//! is *"After"*, and `glossary/After.md` puts that *"immediately after the
//! specified timing point or triggering condition has fully resolved"* — the
//! horror is on her card, and any defeat it caused has been determined, before
//! the damage is dealt.
//!
//! The elder sign is a modifier-only [`Trigger::ElderSign`] reading
//! [`Quantity::HorrorOnController`].
//!
//! [`Trigger::OnEvent`]: card_dsl::dsl::Trigger::OnEvent
//! [`Trigger::ElderSign`]: card_dsl::dsl::Trigger::ElderSign
//! [`EventPattern::HarmPlaced`]: card_dsl::dsl::EventPattern::HarmPlaced
//! [`EventTiming::After`]: card_dsl::dsl::EventTiming::After
//! [`UsageLimit`]: card_dsl::dsl::UsageLimit
//! [`UsagePeriod::Phase`]: card_dsl::dsl::UsagePeriod::Phase
//! [`Quantity::HorrorOnController`]: card_dsl::dsl::Quantity::HorrorOnController

use card_dsl::dsl::{
    self, Ability, EnemyTarget, EventPattern, EventTiming, HarmKind, HarmPlacedScope, IntExpr,
    Quantity, UsageLimit, UsagePeriod,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01004";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Agnes's two printed abilities:
///
/// - `[reaction]` "After 1 or more horror is placed on Agnes Baker: Deal 1
///   damage to an enemy at your location. (Limit once per phase.)"
/// - `[elder_sign]` effect: "+1 for each horror on Agnes Baker."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::reaction_on_event(
            EventPattern::HarmPlaced {
                kind: HarmKind::Horror,
                scope: HarmPlacedScope::YourInvestigator,
            },
            EventTiming::After,
            dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        )
        .with_usage_limit(UsageLimit {
            count: 1,
            period: UsagePeriod::Phase,
        }),
        dsl::elder_sign(IntExpr::Count(Quantity::HorrorOnController)),
    ]
}

#[cfg(test)]
mod tests {
    /// Catches a `CARD` record wired to the wrong code or abilities fn — the
    /// registry must dispatch CODE to this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
