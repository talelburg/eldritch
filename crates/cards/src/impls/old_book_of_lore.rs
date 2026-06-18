//! Old Book of Lore (Seeker item asset, 01031).
//!
//! ```text
//! Item. Tome.
//! [action] Exhaust Old Book of Lore: Choose an investigator at your
//!   location. That investigator searches the top 3 cards of his or her deck
//!   for a card, draws it, and shuffles the remaining cards into his or her
//!   deck.
//! ```
//!
//! One `[action]` ability with an exhaust cost: an
//! [`Effect::SearchDeck`](card_dsl::dsl::Effect::SearchDeck) over the top 3 of
//! a chosen co-located investigator's deck, no filter. "draws it" is modeled as
//! a move to hand (the search primitive's destination); the search shuffles on
//! completion. In solo with one investigator the target auto-binds.

use card_dsl::dsl::{activated, search_deck, Ability, Cost, InvestigatorTarget, SearchScope};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01031";

/// `[action]`, exhaust: a chosen co-located investigator searches the top 3 of
/// their deck for a card, takes it, and shuffles.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![activated(
        1,
        vec![Cost::Exhaust],
        search_deck(
            InvestigatorTarget::chosen_at_your_location(),
            SearchScope::Top(3),
            None,
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Cost, Effect, InvestigatorTarget, SearchScope, Trigger};

    #[test]
    fn abilities_are_action_exhaust_search_top_3() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(abilities[0].costs, vec![Cost::Exhaust]);
        assert!(matches!(
            abilities[0].effect,
            Effect::SearchDeck {
                target: InvestigatorTarget::Chosen(_),
                scope: SearchScope::Top(3),
                filter: None,
            }
        ));
    }
}
