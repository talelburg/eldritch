//! Ward of Protection (Mystic Spell event, 01065).
//!
//! ```text
//! Spell. Spirit.
//! Fast. Play when you draw a non-weakness treachery card.
//! Cancel that card's revelation effect. Then, take 1 horror.
//! ```
//!
//! No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! A Fast event played from hand in the `when` cell of the
//! `EncounterCardDrawn` condition, filtered to a non-weakness treachery drawn
//! by the controller — `glossary/Fast.md`: *"If the instructions specify
//! when/after a timing point, the card may be played as if the described
//! timing point were a triggering condition for playing the card."* The
//! condition's resolve step is the drawn card's revelation, so
//! [`Effect::Cancel`] there prevents it, and the card is still discarded
//! (`glossary/Cancel.md`: *"If the effects of a treachery card are canceled,
//! the card is still regarded as having been drawn, and it is still placed in
//! the encounter discard pile."*).
//!
//! *"Then, take 1 horror"* is dealt through its own `DamageAssigned` and
//! `DamagePlaced` conditions, which resolve normally while the draw stays
//! cancelled: the cancel is a flag on the draw condition's own frame (#1055).
//!
//! [`Effect::Cancel`]: card_dsl::dsl::Effect::Cancel

use card_dsl::card_data::CardType;
use card_dsl::dsl::{self, Ability, CardFilter, Effect, EventPattern, EventTiming};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01065";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities);

/// "Play when you draw a non-weakness treachery card. / Cancel that card's
/// revelation effect. Then, take 1 horror."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::EncounterCardDrawn {
            filter: CardFilter::of_kind(CardType::Treachery).and_non_weakness(),
        },
        EventTiming::When,
        Effect::Seq(vec![
            Effect::Cancel,
            dsl::deal_horror(dsl::InvestigatorTarget::You, 1u8),
        ]),
    )]
}

#[cfg(test)]
mod tests {
    use card_dsl::card_data::CardType;
    use card_dsl::dsl::{CardFilter, EventPattern, EventTiming, Trigger, TriggerKind};

    #[test]
    fn one_when_draw_reaction_narrowed_to_a_non_weakness_treachery() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EncounterCardDrawn {
                    filter: CardFilter {
                        trait_: None,
                        kind: Some(CardType::Treachery),
                        non_weakness: true,
                    },
                },
                timing: EventTiming::When,
                kind: TriggerKind::Reaction,
            },
        );
    }
}
