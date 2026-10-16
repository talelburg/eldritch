//! Psychosis (basic weakness treachery, 01099).
//!
//! ```text
//! Revelation - Add Psychosis to your threat area.
//! Forced - After you take 1 or more horror: Take 1 direct damage.
//! [action] [action]: Discard Psychosis.
//! ```
//!
//! A persistent treachery, like Haunted 01098: the Revelation puts it in the
//! threat area, and the `[action] [action]` discard is reachable by any
//! investigator at the bearer's location. Ruling
//! (<https://arkhamdb.com/card/01099>): *"Any investigator at the same location
//! as the investigator with Psychosis in their threat area may trigger the
//! \[action\]\[action\] to discard Psychosis, as per the FAQ [V1.0, section
//! 2.1]."*
//!
//! **Cell: the `after` cell of the `DamageDealt` condition**, which is what
//! "After you take" names: the Forced is [`EventPattern::HarmTaken`]
//! `{ Horror, You }`. "You" counts horror soaked by the bearer's assets too, which is the
//! other ruling: *"Triggers even if you assign horror to an asset you control
//! (you're still taking horror, even if you reassign it)."* Its damage is
//! direct, so no asset is offered it, and it is a deal like any other: with
//! Hypochondria 01100 also in play the two fire each other until the bearer is
//! defeated.
//!
//! **Agnes Baker 01004 resolves first.** Her *"After 1 or more horror is placed
//! on Agnes Baker"* sits in the placement's `after` cell, which is nested inside
//! the deal (ADR 0009), so it resolves before this Forced in the deal's own
//! `after` cell. No vendored source settles this pair: the order is the one the
//! nesting gives, and it was decided for #781 rather than found in a rule.

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, HarmKind, HarmTakenScope, IntExpr, InvestigatorTarget,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Psychosis.
pub const CODE: &str = "01099";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The Revelation, the Forced, then the `[action] [action]` discard.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::revelation(dsl::put_into_threat_area(CODE)),
        dsl::forced_on_event(
            EventPattern::HarmTaken {
                kind: HarmKind::Horror,
                scope: HarmTakenScope::You,
            },
            EventTiming::After,
            dsl::deal_direct_damage(InvestigatorTarget::You, IntExpr::Lit(1)),
        ),
        dsl::activated(2, vec![], dsl::discard_self()),
    ]
}
