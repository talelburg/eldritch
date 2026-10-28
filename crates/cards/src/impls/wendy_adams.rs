//! Wendy Adams (Survivor investigator, 01005).
//!
//! ```text
//! Wendy Adams. The Urchin.
//! Drifter.
//! Willpower 4. Intellect 3. Combat 1. Agility 4.
//! Health 7. Sanity 7.
//!
//! [reaction] When you reveal a chaos token, choose and discard 1 card from
//! your hand: Cancel that chaos token and return it to the bag. Reveal a new
//! chaos token. (Limit once per test/ability.)
//! [elder_sign] effect: +0. If Wendy's Amulet is in play, you automatically
//! succeed instead.
//! ```
//!
//! # Scope
//!
//! **Cell: the `when` cell of the `ChaosTokenRevealed` condition**, the printed
//! *"When you reveal a chaos token"*. The condition's resolve step is what the
//! token does to the test, so [`Effect::Cancel`] there leaves the token
//! contributing nothing, and [`Effect::RevealChaosToken`] sends the test back to
//! draw again. The token never left the bag, so *"return it to the bag"* needs
//! no step of its own. Her ruling (<https://arkhamdb.com/card/01005>) holds by
//! construction, since a reveal clause is pushed by that same resolve step: *"If
//! you use Wendy's ability to 'cancel a chaos token', treat it as if you've
//! never revealed that token, for purposes of effects that trigger if a specific
//! symbol is revealed (e.g. Baseball Bat)."*
//!
//! The cost is [`Cost::DiscardCardFromHand`], payable only with a non-weakness
//! card in hand, and paid before the effect, so it stays paid when the effect is
//! cancelled. *"Limit once per test/ability"* is a [`UsagePeriod::SkillTest`]
//! limit of one: every chaos token the engine reveals is a skill test's.
//!
//! The elder sign is a `+0` modifier whose `immediate` arm, resolved as the
//! token's symbol effect, latches [`Determination::AutomaticSuccess`] while
//! [`ControlStatus::ByAPlayer`] holds for Wendy's Amulet 01014 — "in play" for
//! a signature only her deck carries. The test still takes place, committed
//! cards included. Her ruling: *"If a skill test automatically succeeds, the
//! total difficulty of that test is considered 0."*
//!
//! [`Effect::Cancel`]: card_dsl::dsl::Effect::Cancel
//! [`Effect::RevealChaosToken`]: card_dsl::dsl::Effect::RevealChaosToken
//! [`Cost::DiscardCardFromHand`]: card_dsl::dsl::Cost::DiscardCardFromHand
//! [`UsagePeriod::SkillTest`]: card_dsl::dsl::UsagePeriod::SkillTest
//! [`Determination::AutomaticSuccess`]: card_dsl::dsl::Determination::AutomaticSuccess
//! [`ControlStatus::ByAPlayer`]: card_dsl::dsl::ControlStatus::ByAPlayer

use card_dsl::dsl::{
    self, Ability, ControlStatus, Cost, Determination, EventPattern, EventTiming, IntExpr,
    TokenOutcome, UsageLimit, UsagePeriod,
};

use crate::impls::{wendys_amulet, CardRecord};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01005";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// Wendy's two printed abilities:
///
/// - `[reaction]` "When you reveal a chaos token, choose and discard 1 card
///   from your hand: Cancel that chaos token and return it to the bag. Reveal a
///   new chaos token. (Limit once per test/ability.)"
/// - `[elder_sign]` effect: "+0. If Wendy's Amulet is in play, you
///   automatically succeed instead."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::reaction_on_event(
            EventPattern::ChaosTokenRevealed,
            EventTiming::When,
            dsl::seq([dsl::cancel(), dsl::reveal_chaos_token()]),
        )
        .with_costs(vec![Cost::DiscardCardFromHand])
        .with_usage_limit(UsageLimit {
            count: 1,
            period: UsagePeriod::SkillTest,
        }),
        dsl::elder_sign_with(TokenOutcome {
            immediate: Some(dsl::if_(
                dsl::control_status(wendys_amulet::CODE, ControlStatus::ByAPlayer),
                dsl::auto_resolve(Determination::AutomaticSuccess),
            )),
            ..TokenOutcome::modifier(IntExpr::Lit(0))
        }),
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
