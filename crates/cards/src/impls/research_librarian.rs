//! Research Librarian (Seeker ally asset, 01032).
//!
//! ```text
//! Ally. Miskatonic.
//! [reaction] After Research Librarian enters play: Search your deck for a
//!   Tome asset and add it to your hand. Shuffle your deck.
//! ```
//!
//! One `EnteredPlay` reaction (self-referential — the engine fires it only for
//! this just-entered instance): an
//! [`Effect::SearchDeck`](card_dsl::dsl::Effect::SearchDeck) over the entire
//! deck, filtered to `Tome` assets, into the controller's hand, then shuffle.
//!
//! # Ally-soak gap
//!
//! Metadata gives Research Librarian `health: 1, sanity: 1` (ally soak, not a
//! stat boost). The DSL doesn't model soak yet (#44), so this impl ships only
//! the reaction; the card is mechanically weaker than printed until soak lands.

use card_dsl::card_data::CardType;
use card_dsl::dsl::{
    reaction_on_event, search_deck, Ability, CardFilter, EventPattern, EventTiming,
    InvestigatorTarget, SearchScope,
};

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01032";

/// "[reaction] After Research Librarian enters play: Search your deck for a
/// Tome asset, add it to your hand, shuffle."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![reaction_on_event(
        EventPattern::EnteredPlay,
        EventTiming::After,
        search_deck(
            InvestigatorTarget::You,
            SearchScope::EntireDeck,
            Some(CardFilter {
                trait_: Some("Tome".into()),
                kind: Some(CardType::Asset),
            }),
        ),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::card_data::CardType;
    use card_dsl::dsl::{
        CardFilter, Effect, EventPattern, EventTiming, InvestigatorTarget, SearchScope, Trigger,
        TriggerKind,
    };

    #[test]
    fn ability_is_entered_play_reaction_search_tome_asset() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredPlay,
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert_eq!(
            abilities[0].effect,
            Effect::SearchDeck {
                target: InvestigatorTarget::You,
                scope: SearchScope::EntireDeck,
                filter: Some(CardFilter {
                    trait_: Some("Tome".into()),
                    kind: Some(CardType::Asset),
                }),
            },
        );
    }
}
