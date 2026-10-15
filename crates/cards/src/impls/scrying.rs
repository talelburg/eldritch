//! Scrying (Mystic spell asset, 01061).
//!
//! ```text
//! Uses (3 charges).
//! [action] Exhaust Scrying and spend 1 charge: Look at the top 3 cards of any
//!   investigator's deck or the encounter deck. Return them to the top of that
//!   deck in any order.
//! ```
//!
//! No rulings: 01061 is listed in `data/arkhamdb-faq/no-rulings.txt`.
//!
//! One `[action]` ability whose costs are the exhaust and the charge. The
//! `Uses (3 charges)` pool is corpus metadata, so it enters play with 3.
//!
//! **Which deck** is an `Effect::ChooseOne` between *"Your deck"* and *"The
//! encounter deck"*, because the later picks must know it and a native cannot
//! hand a pick to the next one: the effect tree holds it instead. Solo, *"any
//! investigator's deck"* is your own; another investigator's deck waits for
//! phase 8's multiplayer work (the #65 family; no issue tracks this choice
//! yet).
//!
//! **The order** is built one card at a time, one native per pick, since a
//! native sees one pick and must make it before it writes. Pick `k` offers the
//! looked-at cards still unplaced, which sit at positions `k..` of the top 3,
//! and moves the chosen one to position `k`
//! ([`NativeCx::move_card_in_deck`]). The last card needs no pick, so there are
//! two per deck, and a deck of fewer than 3 cards asks fewer: one pick for 2
//! cards, none for 0 or 1. The cards are offered by name through
//! [`resolve_card_choice`](engine::resolve_card_choice), the deck search's
//! choice, which is how they are looked at.
//!
//! **The deck itself carries the order between picks.** Looking at a card does
//! not move it, so the three stay in the deck, and there is no zone to hold
//! them aside while the order is built: a holding slot would be the global
//! side-channel ADR 0002 retired for an in-progress play, and a native cannot
//! hand its pick to the next one. So each pick writes straight into the deck,
//! and the next reads where the last left off. **A placed card is never
//! re-offered**: pick `k` offers only positions `k..`, so once a card is
//! placed, its place is final.

use card_dsl::card_data::UseKind;
use card_dsl::dsl::{self, Ability, Cost};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx};
use game_core::state::{CardCode, Deck, GameState};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Scrying (original-Core printing).
pub const CODE: &str = "01061";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[
    (YOUR_DECK_FIRST, your_deck_first),
    (YOUR_DECK_SECOND, your_deck_second),
    (ENCOUNTER_DECK_FIRST, encounter_deck_first),
    (ENCOUNTER_DECK_SECOND, encounter_deck_second),
]);

const YOUR_DECK_FIRST: &str = "01061:your-deck-first";
const YOUR_DECK_SECOND: &str = "01061:your-deck-second";
const ENCOUNTER_DECK_FIRST: &str = "01061:encounter-deck-first";
const ENCOUNTER_DECK_SECOND: &str = "01061:encounter-deck-second";

/// How many cards from the top Scrying looks at.
const LOOKED_AT: usize = 3;

/// `[action]` Exhaust + 1 charge: choose a deck, then put its top 3 back in
/// the order picked.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::activated(
        1,
        vec![
            Cost::Exhaust,
            Cost::SpendUses {
                kind: UseKind::Charges,
                count: 1,
            },
        ],
        dsl::choose_one(vec![
            (
                "Your deck",
                dsl::seq([dsl::native(YOUR_DECK_FIRST), dsl::native(YOUR_DECK_SECOND)]),
            ),
            (
                "The encounter deck",
                dsl::seq([
                    dsl::native(ENCOUNTER_DECK_FIRST),
                    dsl::native(ENCOUNTER_DECK_SECOND),
                ]),
            ),
        ]),
    )]
}

fn your_deck_first(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next(cx, ctx, Deck::Investigator(ctx.controller), 0)
}

fn your_deck_second(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next(cx, ctx, Deck::Investigator(ctx.controller), 1)
}

fn encounter_deck_first(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next(cx, ctx, Deck::Encounter, 0)
}

fn encounter_deck_second(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next(cx, ctx, Deck::Encounter, 1)
}

/// Pick which of the looked-at cards still unplaced goes `position` cards
/// below the top, and move it there. Nothing to pick when one card or none is
/// left.
fn place_next(cx: &mut NativeCx, ctx: &EvalContext, deck: Deck, position: usize) -> EngineOutcome {
    let unplaced = unplaced(cx.state(), deck, position);
    if unplaced.len() < 2 {
        return EngineOutcome::Done;
    }
    let prompt = match position {
        0 => "Scrying: choose the card to put on top",
        _ => "Scrying: choose the card to put next",
    };
    match engine::resolve_card_choice(cx.state(), cx.cards(), ctx, &unplaced, prompt) {
        Grounded::Picked(i) => {
            cx.move_card_in_deck(deck, position + i, position);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}

/// The looked-at cards from `position` down: positions `position..3` of
/// `deck`, cut short by a deck of fewer than 3 cards.
fn unplaced(state: &GameState, deck: Deck, position: usize) -> Vec<CardCode> {
    let range = position..LOOKED_AT;
    match deck {
        Deck::Investigator(id) => state.investigators.get(&id).map_or_else(Vec::new, |inv| {
            inv.deck
                .iter()
                .take(range.end)
                .skip(range.start)
                .cloned()
                .collect()
        }),
        Deck::Encounter => state
            .encounter_deck
            .iter()
            .take(range.end)
            .skip(range.start)
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{Effect, Trigger};

    use super::*;

    #[test]
    fn one_action_exhausting_and_spending_a_charge_to_choose_a_deck() {
        let abilities = abilities();
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, Trigger::Activated { action_cost: 1 });
        assert_eq!(abilities[0].designator, None);
        assert_eq!(
            abilities[0].costs,
            vec![
                Cost::Exhaust,
                Cost::SpendUses {
                    kind: UseKind::Charges,
                    count: 1,
                },
            ]
        );
        let Effect::ChooseOne(branches) = &abilities[0].effect else {
            panic!("expected ChooseOne, got {:?}", abilities[0].effect);
        };
        let labels: Vec<_> = branches.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Your deck", "The encounter deck"]);
    }

    /// Catches a `CARD` record wired to the wrong code or abilities fn —
    /// the registry must dispatch CODE here.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(CODE), Some(abilities()));
    }
}
