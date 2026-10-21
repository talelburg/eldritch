//! Mob Enforcer (basic weakness enemy, 01101).
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! [action] Spend 4 resources: Parley. Discard Mob Enforcer.
//! ```
//!
//! `Prey` and `Hunter` are printed keywords the pipeline ingests into metadata
//! (`Prey::BearerOnly`), not abilities; only the Parley is declared here.
//!
//! **The Parley is an ability on the enemy**, reached by any investigator at
//! its location (`AbilitySource::Enemy`). Its cost is the 4 resources, which
//! refuse the activation when the activating investigator cannot pay them, and
//! its effect is [`Effect::DiscardSelf`](card_dsl::dsl::Effect::DiscardSelf):
//! the enemy leaves play by its owner, so it goes to its bearer's discard pile.
//!
//! **Discarding it is not defeating it.** Ruling
//! (`data/arkhamdb-faq/core/01101.md`, <https://arkhamdb.com/card/01101>):
//! *"Discarding an enemy is not the same as defeating it, so reactions to an
//! enemy being defeated (e.g. Roland Banks, Evidence!) do not trigger."* The
//! discard exit logs no `EnemyDefeated`, so no defeat reaction has anything to
//! fire on.

use card_dsl::dsl::{self, Ability, ActionDesignator, Cost};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Mob Enforcer.
pub const CODE: &str = "01101";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated_as(
        ActionDesignator::Parley,
        1,
        vec![Cost::Resources(4)],
        dsl::discard_self(),
    )]
}
