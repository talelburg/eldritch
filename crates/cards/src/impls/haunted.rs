//! Haunted (basic weakness treachery, 01098).
//!
//! ```text
//! Revelation - Add Haunted to your threat area.
//! You get -1 to each of your skills.
//! [action] [action]: Discard Haunted.
//! ```
//!
//! A persistent treachery: it has abilities besides its Revelation, so it stays
//! in the threat area the Revelation puts it in.
//!
//! *"-1 to each of your skills"* is four constant [`Effect::Modify`]s, one per
//! skill, since there is no all-skills [`Stat`]. They are aimed at the
//! controller, which for a threat-area card is the investigator whose threat
//! area it is in (`glossary/You_Your.md`: *"the investigator who has the card in
//! his/her threat area"*), and read by the modifier sweep.
//!
//! The `[action] [action]` discard needs nothing card-local to reach a second
//! investigator. Its ruling (<https://arkhamdb.com/card/01098>): *"Any
//! investigator at the same location as the investigator with Haunted in their
//! threat area may trigger the [action][action] to discard Haunted, as per the
//! FAQ [V1.0, section 2.1]."* The engine's ability-source reach already admits a
//! co-located investigator's threat-area encounter-cardtype card, and the
//! discard goes to the owner's pile (#981).
//!
//! [`Effect::Modify`]: card_dsl::dsl::Effect::Modify

use card_dsl::dsl::{self, Ability, ModifierScope, Stat};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Haunted.
pub const CODE: &str = "01098";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// The Revelation, the four skill constants, then the `[action] [action]`
/// discard, in that order.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    let mut abilities = vec![dsl::revelation(dsl::put_into_threat_area(CODE))];
    abilities.extend(
        [
            Stat::Willpower,
            Stat::Intellect,
            Stat::Combat,
            Stat::Agility,
        ]
        .map(|stat| dsl::constant(dsl::modify(stat, -1, ModifierScope::WhileInPlay))),
    );
    abilities.push(dsl::activated(2, vec![], dsl::discard_self()));
    abilities
}
