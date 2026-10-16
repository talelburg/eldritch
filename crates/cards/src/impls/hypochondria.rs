//! Hypochondria (basic weakness treachery, 01100).
//!
//! ```text
//! Revelation - Add Hypochondria to your threat area.
//! Forced - After you take 1 or more damage: Take 1 direct horror.
//! [action] [action]: Discard Hypochondria.
//! ```
//!
//! The mirror of Psychosis 01099, with damage and horror swapped. Rulings
//! (<https://arkhamdb.com/card/01100>): *"Any investigator at the same location
//! as the investigator with Hypochondria in their threat area may trigger the
//! [action][action] to discard Hypochondria, as per the FAQ [V1.0, section
//! 2.1]."* and *"Triggers even if you assign damage to an asset you control
//! (you're still taking damage, even if you reassign it)."*
//!
//! The Forced is [`EventPattern::HarmTaken`] `{ Damage, You }` in the `after`
//! cell, and its horror is direct.

use card_dsl::dsl::{
    self, Ability, EventPattern, EventTiming, HarmKind, HarmTakenScope, IntExpr, InvestigatorTarget,
};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Hypochondria.
pub const CODE: &str = "01100";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The Revelation, the Forced, then the `[action] [action]` discard.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::revelation(dsl::put_into_threat_area(CODE)),
        dsl::forced_on_event(
            EventPattern::HarmTaken {
                kind: HarmKind::Damage,
                scope: HarmTakenScope::You,
            },
            EventTiming::After,
            dsl::deal_direct_horror(InvestigatorTarget::You, IntExpr::Lit(1)),
        ),
        dsl::activated(2, vec![], dsl::discard_self()),
    ]
}
