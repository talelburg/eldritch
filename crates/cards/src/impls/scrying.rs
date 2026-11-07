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
//! **Which deck** is an `Effect::ChooseOne` between *"An investigator's deck"*
//! and *"The encounter deck"*, because the later picks must know it and a
//! native cannot hand a pick to the next one: the effect tree holds it
//! instead. The investigator-deck branch then chooses the investigator with an
//! `Effect::ChooseInvestigator` over every investigator in play, which keeps
//! the pick for both of the branch's natives
//! ([`EvalContext::kept_investigator`]). With one investigator left in the
//! game, that choice binds without a prompt outside interactive play.
//!
//! **The order** is built one card at a time, one native per pick, since a
//! native sees one pick and must make it before it writes. Pick `k` offers the
//! looked-at cards still unplaced, which sit at positions `k..` of the top 3,
//! and moves the chosen one to position `k`
//! ([`NativeCx::move_card_in_deck`]). The last card needs no pick, so there are
//! two per deck, and a deck of fewer than 3 cards asks fewer: one pick for 2
//! cards, none for 0 or 1. The cards are offered through
//! [`resolve_card_choice`](engine::resolve_card_choice), the deck search's
//! choice, which is how they are looked at: each anchored by its position in
//! the deck, and the prompt by every card looked at
//! ([`OptionTarget::LookedAtTopOfDeck`] or
//! [`OptionTarget::LookedAtTopOfEncounterDeck`]), which is not a search
//! (FAQ 2.13 in `glossary/Search.md`).
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
use card_dsl::dsl::{self, Ability, Cost, LocationSet};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{Deck, DeckIndex, GameState};

use crate::impls::CardRecord;

/// `ArkhamDB` code for Scrying (original-Core printing).
pub const CODE: &str = "01061";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities).effects(&[
    (INVESTIGATOR_DECK_FIRST, investigator_deck_first),
    (INVESTIGATOR_DECK_SECOND, investigator_deck_second),
    (ENCOUNTER_DECK_FIRST, encounter_deck_first),
    (ENCOUNTER_DECK_SECOND, encounter_deck_second),
]);

const INVESTIGATOR_DECK_FIRST: &str = "01061:investigator-deck-first";
const INVESTIGATOR_DECK_SECOND: &str = "01061:investigator-deck-second";
const ENCOUNTER_DECK_FIRST: &str = "01061:encounter-deck-first";
const ENCOUNTER_DECK_SECOND: &str = "01061:encounter-deck-second";

/// How many cards from the top Scrying looks at.
const LOOKED_AT: usize = 3;

/// `[action]` Exhaust + 1 charge: choose a deck (for an investigator's deck,
/// then the investigator), then put its top 3 back in the order picked.
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
                "An investigator's deck",
                dsl::choose_investigator(
                    LocationSet::Anywhere,
                    dsl::seq([
                        dsl::native(INVESTIGATOR_DECK_FIRST),
                        dsl::native(INVESTIGATOR_DECK_SECOND),
                    ]),
                ),
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

fn investigator_deck_first(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next_in_kept_deck(cx, ctx, 0)
}

fn investigator_deck_second(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    place_next_in_kept_deck(cx, ctx, 1)
}

/// [`place_next`] in the deck of the investigator the enclosing
/// `ChooseInvestigator` kept.
fn place_next_in_kept_deck(cx: &mut NativeCx, ctx: &EvalContext, position: usize) -> EngineOutcome {
    match ctx.kept_investigator() {
        Some(id) => place_next(cx, ctx, Deck::Investigator(id), position),
        None => EngineOutcome::Rejected {
            reason: "Scrying: an investigator's deck outside a ChooseInvestigator".into(),
        },
    }
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
///
/// Each card is anchored by its position in the deck, and the prompt by every
/// card looked at, the placed ones included, so a host can show the order
/// built so far.
fn place_next(cx: &mut NativeCx, ctx: &EvalContext, deck: Deck, position: usize) -> EngineOutcome {
    let looked_at = looked_at(cx.state(), deck);
    if looked_at.saturating_sub(position) < 2 {
        return EngineOutcome::Done;
    }
    let unplaced: Vec<OptionTarget> = (position..looked_at)
        .map(|i| OptionTarget::deck_card(deck, DeckIndex::new(i)))
        .collect();
    let area = OptionTarget::looked_at_top_of(
        deck,
        u8::try_from(looked_at).expect("Scrying looks at 3 cards at most"),
    );
    let prompt = match position {
        0 => "Scrying: choose the card to put on top",
        _ => "Scrying: choose the card to put next",
    };
    match engine::resolve_card_choice(cx.state(), cx.cards(), ctx, &unplaced, area, prompt) {
        Grounded::Picked(i) => {
            cx.move_card_in_deck(deck, position + i, position);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(suspension) => cx.suspend(suspension),
    }
}

/// How many cards Scrying looks at in `deck`: 3, or fewer when the deck
/// holds fewer.
fn looked_at(state: &GameState, deck: Deck) -> usize {
    let held = match deck {
        Deck::Investigator(id) => state.investigators.get(&id).map_or(0, |inv| inv.deck.len()),
        Deck::Encounter => state.encounter_deck.len(),
    };
    held.min(LOOKED_AT)
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
        assert_eq!(labels, ["An investigator's deck", "The encounter deck"]);
        assert_eq!(
            branches[0].effect,
            dsl::choose_investigator(
                LocationSet::Anywhere,
                dsl::seq([
                    dsl::native(INVESTIGATOR_DECK_FIRST),
                    dsl::native(INVESTIGATOR_DECK_SECOND),
                ]),
            ),
            "the investigator-deck branch chooses any investigator, then places in their deck",
        );
    }
}
