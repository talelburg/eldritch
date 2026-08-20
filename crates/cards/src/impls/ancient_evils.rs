//! Ancient Evils (The Gathering treachery, 01166).
//!
//! ```text
//! Revelation - Place 1 doom on the current agenda. This effect can cause
//!   the current agenda to advance.
//! ```
//!
//! The clause is a DSL primitive as of #716
//! ([`Effect::PlaceDoomOnCurrentAgenda`](card_dsl::dsl::Effect::PlaceDoomOnCurrentAgenda)),
//! which places the doom and runs the threshold check — the *"can cause the
//! current agenda to advance"* half. It reached the repo's two-consumer bar
//! (`CLAUDE.md`, Architecture) against Silver Twilight Acolyte 01102's
//! byte-identical native tag, and four more in-corpus cards print it, three of
//! them inside a `Seq` or `ChooseOne` where a native tag cannot go.

use card_dsl::dsl::{place_doom_on_current_agenda, revelation, Ability};

/// `ArkhamDB` code for Ancient Evils.
pub const CODE: &str = "01166";

#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![revelation(place_doom_on_current_agenda(1u8))]
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
                count: IntExpr::Lit(1)
            },
            "the card prints `Place 1 doom on the current agenda`"
        );
    }
}
