//! Arcane Initiate (Mystic ally asset, 01063).
//!
//! ```text
//! Ally. Sorcerer.
//! Forced - After Arcane Initiate enters play: Place 1 doom on it.
//! [fast] Exhaust Arcane Initiate: Search the top 3 cards of your deck for a
//!   [[Spell]] card and draw it. Shuffle your deck.
//! ```
//!
//! **The forced** rides the self-referential `EnteredPlay` pattern in its
//! `after` cell (the printed *"After"*), and places the doom through a
//! card-local native on the firing instance — one consumer, so no DSL
//! primitive. The doom lives on the instance, so it counts toward the agenda's
//! threshold while the Initiate is in play and leaves play with it
//! (`glossary/Doom.md`: *"Doom on cards other than the agenda (such as enemies,
//! allies, locations, etc.) counts towards the amount of doom in play."*).
//!
//! **The search** is [`Effect::SearchDeck`](card_dsl::dsl::Effect::SearchDeck)
//! over the top 3 with a Spell filter. "Draw it" is modeled as the search's
//! move to hand, as for Old Book of Lore 01031. The primitive already holds the
//! ruling (<https://arkhamdb.com/card/01063>): *"if there is a **Spell** among
//! them, you must draw it, even if it is Dark Memory. You cannot choose to fail
//! to find what you're searching for if it's there."* — one eligible card is
//! taken outright, two or more prompt a pick with no decline, and the deck is
//! shuffled either way.
//!
//! Ally soak (health 1, sanity 2) rides on metadata.

use card_dsl::dsl::{
    self, Ability, CardFilter, Cost, EventPattern, EventTiming, InvestigatorTarget, SearchScope,
};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::generated::traits::SPELL;
use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01063";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[(PLACE_DOOM, place_doom)]);

const PLACE_DOOM: &str = "01063:place-doom";

/// The forced doom placement, then the `[fast]` Spell search, in printed order.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![
        dsl::forced_on_event(
            EventPattern::EnteredPlay,
            EventTiming::After,
            dsl::native(PLACE_DOOM),
        ),
        dsl::activated(
            0,
            vec![Cost::Exhaust],
            dsl::search_deck(
                InvestigatorTarget::You,
                SearchScope::Top(3),
                Some(CardFilter::with_trait(SPELL)),
            ),
        ),
    ]
}

/// *"Place 1 doom on it."* — on the instance that just entered play.
fn place_doom(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let Some(source) = ctx.source_instance() else {
        return EngineOutcome::Rejected {
            reason: "arcane_initiate place doom: no source instance".into(),
        };
    };
    cx.place_doom_on_card(source, 1);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{
        CardFilter, Cost, Effect, EventPattern, EventTiming, InvestigatorTarget, SearchScope,
        Trigger, TriggerKind,
    };

    use crate::generated::traits::SPELL;

    #[test]
    fn abilities_are_the_forced_doom_then_the_fast_spell_search() {
        let abilities = super::abilities();
        assert_eq!(abilities.len(), 2);
        assert_eq!(
            abilities[0].trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnteredPlay,
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            },
        );
        assert_eq!(abilities[1].trigger, Trigger::Activated { action_cost: 0 });
        assert_eq!(abilities[1].designator, None);
        assert_eq!(abilities[1].costs, vec![Cost::Exhaust]);
        assert_eq!(
            abilities[1].effect,
            Effect::SearchDeck {
                target: InvestigatorTarget::You,
                scope: SearchScope::Top(3),
                filter: Some(CardFilter {
                    trait_: Some(SPELL.into()),
                    kind: None,
                    non_weakness: false,
                }),
            },
        );
    }
}
