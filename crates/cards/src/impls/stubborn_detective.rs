//! Stubborn Detective (basic weakness enemy, 01103).
//!
//! ```text
//! Prey - Bearer only.
//! Hunter.
//! While Stubborn Detective is at your location, treat your investigator as if
//! his or her printed text box were blank (except for [[Traits]]).
//! ```
//!
//! `Prey` and `Hunter` are printed keywords the pipeline ingests into metadata
//! (`Prey::BearerOnly`), not abilities; only the blank is declared here.
//!
//! **The blank is a constant the engine reads**, [`Effect::BlankText`]: the
//! ability lookup that answers what is printed on an investigator card sweeps
//! the board for it and answers nothing for a blanked one. Gained text
//! survives (`glossary/Blank.md`: *"Text and/or icons gained from another
//! source are not blanked."*), and skills, health, sanity and traits are not
//! abilities, so they read unchanged.
//!
//! **"Your" is each investigator at his location**, not just his bearer.
//! `glossary/You_Your.md`: *"This clause identifies "you" as any investigator
//! at his location."* Ruling (`data/arkhamdb-faq/core/01103.md`,
//! <https://arkhamdb.com/card/01103>): *"His static ability affects each
//! investigator in his location (not just the bearer)."*
//!
//! **Defeating him un-blanks the defeater in time for their reaction.** Ruling:
//! *"Roland's ability can be used as normal after you defeat this enemy."* He
//! leaves play before the after-defeat window is scanned, so no code here
//! serves it.
//!
//! **The elder sign is blanked with the rest**, by the user's decision rather
//! than the sources'. `modified_value::elder_sign_outcome` in `game-core` has
//! the reasoning.
//!
//! [`Effect::BlankText`]: card_dsl::dsl::Effect::BlankText

use card_dsl::dsl::{self, Ability, BlankTarget};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Stubborn Detective.
pub const CODE: &str = "01103";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::constant(dsl::blank_text(
        BlankTarget::InvestigatorsAtThisLocation,
    ))]
}
