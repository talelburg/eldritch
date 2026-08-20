//! Ancient Evils (The Gathering treachery, 01166).
//!
//! ```text
//! Revelation - Place 1 doom on the current agenda. This effect can cause
//!   the current agenda to advance.
//! ```
//!
//! The clause is a DSL primitive as of #716
//! ([`Effect::PlaceDoomOnCurrentAgenda`](card_dsl::dsl::Effect::PlaceDoomOnCurrentAgenda)).
//! It reached the repo's two-consumer bar (`CLAUDE.md`, Architecture) against
//! Silver Twilight Acolyte 01102's byte-identical native tag, and the corpus
//! prints a doom placement in four more places — three of them nested inside a
//! `ChooseOne` (Offer of Power 01178), a `Seq` (Saracenic Script 02240's act
//! back), or an `If`-on-failure (Blood on the Altar 02195's `[elder_thing]`),
//! where a native tag cannot go.
//!
//! **This card prints the second sentence, so its doom can advance the agenda
//! immediately.** `data/rules-reference/rules/glossary/Doom.md` makes that a
//! privilege rather than the default:
//!
//! > Unless a card otherwise specifies that it can advance the agenda, this is
//! > the only time at which the agenda can advance.
//!
//! *"This"* being the Mythos phase's check-doom-threshold step. 01166 does
//! otherwise specify, so it builds with
//! [`place_doom_that_can_advance_the_agenda`] rather than the bare
//! [`place_doom_on_current_agenda`](card_dsl::dsl::place_doom_on_current_agenda)
//! that 01102 uses.

use card_dsl::dsl::{place_doom_that_can_advance_the_agenda, revelation, Ability};

/// `ArkhamDB` code for Ancient Evils.
pub const CODE: &str = "01166";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(place_doom_that_can_advance_the_agenda(1u8))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_dsl::dsl::{Effect, IntExpr, Trigger};

    #[test]
    fn revelation_places_one_doom_on_the_current_agenda() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Revelation);
        assert_eq!(
            abilities[0].effect,
            Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: true,
            },
            "the card prints `Place 1 doom on the current agenda. This effect \
             can cause the current agenda to advance.` — the second sentence \
             is `may_advance`"
        );
    }
}
