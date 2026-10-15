//! Hospital Debts ("Skids" O'Toole signature weakness, 01011).
//!
//! ```text
//! Revelation - Put Hospital Debts into play in your threat area.
//! [fast]: Move 1 resource from your resource pool to Hospital Debts. (Limit
//!   twice per round.)
//! Forced - When the game ends, if Hospital Debts has fewer than 6 resources
//!   on it: You earn 2 fewer experience for this scenario.
//! ```
//!
//! Persistent treachery: the Revelation self-places into the threat area
//! (`Effect::PutIntoThreatArea`), so `resolve_encounter_card` does not discard
//! it. The `[fast]` ability is a zero-cost activation whose native moves the
//! resource through [`NativeCx::move_resources_to_card`]: a **move**, so it
//! announces neither a gain nor a payment, and the count sits on
//! `CardInPlay::resources`.
//!
//! **Anyone standing with "Skids" may trigger it.** Ruling
//! (<https://arkhamdb.com/card/01011>): *"Any investigator at the same location
//! as "Skids" O'Toole with Hospital Debts in his threat area may trigger the
//! \[lighting\] to add resources to Hospital Debts, as per the FAQ \[V1.0, section
//! 2.1\]."* The reachability predicate already reaches a co-located threat-area
//! treachery, and "your resource pool" is the triggering investigator's: the
//! ability's controller is its activator.
//!
//! **The limit is each investigator's own.** `glossary/Limits_and_Maximums.md`:
//! *"Unless stated otherwise, limits are player specific."* — so Skids and a
//! friend beside him get two triggers a round each; the engine keys usage by
//! card instance and triggering investigator.
//!
//! **An empty pool offers nothing.** The native is opaque to the change-state
//! check, so `HAS_RESOURCE_TAG` gates the ability on the triggering
//! investigator holding at least 1 resource, Cover Up 01007's `HAS_CLUES_TAG`
//! pattern.
//!
//! # Module gap
//!
//! TODO(#1062): the Forced experience clause, *"When the game ends, if
//! Hospital Debts has fewer than 6 resources on it: You earn 2 fewer experience
//! for this scenario."*, waits on campaign experience. Its second ruling — *"If
//! "Skids" is eliminated (by being defeated or taking a **resign** action)
//! while Hospital Debts is in play, Hospital Debts' **Forced** effect triggers,
//! …"* — will follow Cover Up 01007's elimination path when it lands.

use card_dsl::dsl::{self, Ability, UsageLimit, UsagePeriod};
use game_core::engine::board::MoveResourcesError;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};
use game_core::state::GameState;

use crate::impls::CardRecord;

/// `ArkhamDB` code for Hospital Debts.
pub const CODE: &str = "01011";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[(MOVE_TAG, move_resource)])
    .conditions(&[(HAS_RESOURCE_TAG, has_resource)]);

/// Native tag: move 1 resource from the triggerer's pool onto Hospital Debts.
const MOVE_TAG: &str = "01011:move-resource";
/// Native-condition tag: the triggering investigator has a resource to move.
const HAS_RESOURCE_TAG: &str = "01011:has-resource";

/// Hospital Debts' printed abilities, the Forced clause aside (see the module
/// gap):
///
/// - *"Revelation - Put Hospital Debts into play in your threat area."*
/// - `[fast]` *"Move 1 resource from your resource pool to Hospital Debts.
///   (Limit twice per round.)"*
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::revelation(dsl::put_into_threat_area(CODE)),
        dsl::activated(0, vec![], dsl::native(MOVE_TAG))
            .with_usage_limit(UsageLimit {
                count: 2,
                period: UsagePeriod::Round,
            })
            .with_eligibility(dsl::native_condition(HAS_RESOURCE_TAG)),
    ]
}

/// True while the triggering investigator has at least 1 resource to move.
fn has_resource(state: &GameState, ctx: &EvalContext) -> bool {
    state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| inv.resources > 0)
}

/// *"Move 1 resource from your resource pool to Hospital Debts."* "Your" is
/// the triggering investigator. A Hospital Debts no longer in play has nowhere
/// to put it.
fn move_resource(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source_instance() else {
        return EngineOutcome::Rejected {
            reason: "hospital_debts: no source instance".into(),
        };
    };
    match cx.move_resources_to_card(ctx.controller, source, 1) {
        Ok(()) | Err(MoveResourcesError::NotInPlay) => EngineOutcome::Done,
        Err(err) => EngineOutcome::Rejected {
            reason: format!("hospital_debts: moving a resource failed: {err:?}").into(),
        },
    }
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
