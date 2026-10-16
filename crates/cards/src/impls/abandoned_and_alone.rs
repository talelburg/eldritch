//! Abandoned and Alone (Wendy Adams' signature weakness treachery, 01015).
//!
//! ```text
//! Revelation - Take 2 direct horror and remove all cards in your discard pile
//!   from the game.
//! ```
//!
//! A one-shot treachery: its Revelation is its only ability, so once it
//! resolves the card goes to its bearer's discard pile. That discard comes
//! after the Revelation, so the card is not among those it removes, and it is
//! alone in the pile afterwards.
//!
//! The Revelation is a sequence: the 2 direct horror through
//! [`dsl::deal_direct_horror`], so no asset is offered it
//! (`glossary/Direct_Damage_Direct_Horror.md`: *"that damage or horror must be
//! assigned directly to the specified card, and cannot be assigned or
//! re-assigned elsewhere."*), then a card-local native over
//! [`NativeCx::remove_discard_pile_from_game`], a single consumer of "remove
//! your discard pile from the game" (`docs/agents/standards.md`, the
//! DSL-primitive threshold). The removal waits for the deal to resolve, so if
//! the horror defeats the bearer, elimination has already removed the pile and
//! the native finds it empty.
//!
//! The ruling (<https://arkhamdb.com/card/01015>): *"Remove cards from the game
//! for the duration of the current scenario only. They're still part of your
//! deck during this campaign."* Nothing carries a deck from one scenario to the
//! next yet, so the removal is the scenario's; returning the cards to the deck
//! between scenarios is phase 9's campaign work.

use card_dsl::dsl::{self, Ability, InvestigatorTarget};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Abandoned and Alone.
pub const CODE: &str = "01015";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord =
    CardRecord::new(CODE, abilities).effects(&[(REMOVE_DISCARD_PILE, remove_discard_pile)]);

/// Native tag: remove every card in the drawer's discard pile from the game.
const REMOVE_DISCARD_PILE: &str = "01015:remove-discard-pile";

/// "**Revelation** - Take 2 direct horror and remove all cards in your discard
/// pile from the game."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::revelation(dsl::seq([
        dsl::deal_direct_horror(InvestigatorTarget::You, 2_u8),
        dsl::native(REMOVE_DISCARD_PILE),
    ]))]
}

fn remove_discard_pile(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.remove_discard_pile_from_game(ctx.controller);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
