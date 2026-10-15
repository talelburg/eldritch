//! "Skids" O'Toole (Rogue investigator, 01003).
//!
//! ```text
//! "Skids" O'Toole. The Ex-Con.
//! Criminal.
//! Willpower 2. Intellect 3. Combat 3. Agility 4.
//! Health 8. Sanity 6.
//!
//! [fast] During your turn, spend 2 resources: You may take an additional
//! action this turn. (Limit once per turn.)
//! [elder_sign] effect: +2. If you succeed, gain 2 resources.
//! ```
//!
//! # Scope
//!
//! Both printed abilities, in Roland Banks 01001's registration shape. His one
//! ruling (<https://arkhamdb.com/card/01003>) is about Parallel Skids'
//! deckbuilding and touches neither.
//!
//! The `[fast]` ability is a zero-action activated ability costing
//! [`Cost::Resources`]`(2)`, whose effect is an unrestricted
//! [`Effect::GainAdditionalAction`], an additional action this turn.
//! *"(Limit once per turn.)"* is a [`UsageLimit`] with [`UsagePeriod::Turn`].
//!
//! *"During your turn"* is the ability's
//! [`eligibility`](card_dsl::dsl::Ability::eligibility), a
//! [`Condition::Native`](card_dsl::dsl::Condition::Native) predicate: it is
//! the slot for a play restriction the engine cannot derive, and the only card
//! that needs this one is Skids. ADR 0017 keeps *timing* out of the initiation
//! gate, but this clause is a restriction printed on the ability rather than
//! the timing of a path, which is what the eligibility slot holds.
//!
//! The elder sign is [`dsl::elder_sign_with`] a +2 modifier and an `on_success`
//! arm, which resolves at Skill Test Timing step 7, after the action's own
//! result (the Investigate's clues, the Fight's damage).
//!
//! [`Cost::Resources`]: card_dsl::dsl::Cost::Resources
//! [`Effect::GainAdditionalAction`]: card_dsl::dsl::Effect::GainAdditionalAction

use card_dsl::dsl::{
    self, Ability, Cost, IntExpr, InvestigatorTarget, TokenOutcome, UsageLimit, UsagePeriod,
};
use game_core::engine::evaluator::EvalContext;
use game_core::state::{GameState, Phase};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01003";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).conditions(&[(DURING_YOUR_TURN_TAG, during_your_turn)]);

/// Native-condition tag: it is the controller's turn.
const DURING_YOUR_TURN_TAG: &str = "01003:during-your-turn";

/// Skids's two printed abilities:
///
/// - `[fast]` "During your turn, spend 2 resources: You may take an additional
///   action this turn. (Limit once per turn.)"
/// - `[elder_sign]` effect: "+2. If you succeed, gain 2 resources."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::activated(0, vec![Cost::Resources(2)], dsl::gain_additional_action())
            .with_usage_limit(UsageLimit {
                count: 1,
                period: UsagePeriod::Turn,
            })
            .with_eligibility(dsl::native_condition(DURING_YOUR_TURN_TAG)),
        dsl::elder_sign_with(TokenOutcome {
            modifier: IntExpr::Lit(2),
            on_success: Some(dsl::gain_resources(InvestigatorTarget::You, 2)),
            ..TokenOutcome::default()
        }),
    ]
}

/// True during the controller's turn: the Investigation phase, with them the
/// active investigator. The end of the turn is still part of it, as the
/// play-side "play only during your turn" gate argues from the official FAQ's
/// Agatha Crane answer (`data/official-faq/Frequently_Asked_Questions.md`):
/// *"You resolve Agatha Crane’s ability at the end of your turn, which is still
/// during your turn."*
fn during_your_turn(state: &GameState, ctx: &EvalContext) -> bool {
    state.phase == Phase::Investigation && state.active_investigator == Some(ctx.controller)
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
