//! The choice row: a choice among cards outside play, as every account that
//! doesn't answer it sees it, on the chooser's tab and with no controls.
//!
//! [`crate::prompt_layout::layout`] decides who gets the row
//! ([`CardChoice::Row`](crate::prompt_layout::CardChoice::Row)); this module
//! decides what it shows, from the choice's [`CardArea`] alone. A search keeps
//! its cards hidden: `glossary/Search.md`, *"that player is permitted to look
//! at all of the cards in the searched area without revealing those cards to
//! the other players."* So the row of a search carries no card code, and none
//! reaches the DOM.
//!
//! Scrying 01061's looked-at cards are hidden the same way, though looking at
//! is not a search (FAQ 2.13, *"if an investigator "looks at" the top 3 cards
//! of their deck, this is not a search effect"*): a product choice, to play as
//! the table does in person (#1243).
//!
//! [`ChoiceRow`] is pure and native-tested.

use game_core::state::{GameState, InvestigatorId};
use leptos::prelude::*;

use crate::card_modal::CardArea;

/// What the row shows: a caption and the cards under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    /// What is happening to the cards, as the row's caption reads it: "being
    /// searched".
    pub caption: String,
    /// The cards, as the row lays them out.
    pub cards: RowCards,
}

/// Which back a face-down card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// A player card's back: an investigator's deck.
    Player,
    /// An encounter card's back: the encounter deck.
    Encounter,
}

impl Back {
    /// The face-down card's CSS classes.
    #[must_use]
    pub fn class(self) -> &'static str {
        match self {
            Back::Player => "deck-back",
            Back::Encounter => "deck-back encounter-back",
        }
    }
}

/// How the row lays the area's cards out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowCards {
    /// This many cards, each face down, showing `Back`.
    FaceDown(usize, Back),
    /// One face-down deck, standing for the whole of one. The caption carries
    /// its count.
    Deck,
}

impl ChoiceRow {
    /// The row for a choice among `area`'s cards in `game`. The caption comes
    /// from the area's kind, never the prompt's text.
    #[must_use]
    pub fn of(area: CardArea, game: &GameState) -> Self {
        match area {
            CardArea::SearchedTopOfDeck { count, .. } => ChoiceRow {
                caption: "being searched".to_owned(),
                cards: RowCards::FaceDown(count, Back::Player),
            },
            CardArea::SearchedDeck(investigator) => ChoiceRow {
                caption: format!("being searched: {}", cards(deck_size(game, investigator))),
                cards: RowCards::Deck,
            },
            CardArea::LookedAtTopOfDeck { count, .. } => ChoiceRow {
                caption: "being looked at".to_owned(),
                cards: RowCards::FaceDown(count, Back::Player),
            },
            CardArea::LookedAtTopOfEncounterDeck { count } => ChoiceRow {
                caption: "being looked at".to_owned(),
                cards: RowCards::FaceDown(count, Back::Encounter),
            },
        }
    }
}

/// How many cards `investigator`'s deck holds in `game`.
fn deck_size(game: &GameState, investigator: InvestigatorId) -> usize {
    game.investigators
        .get(&investigator)
        .map_or(0, |inv| inv.deck.len())
}

/// "1 card", "27 cards".
fn cards(count: usize) -> String {
    match count {
        1 => "1 card".to_owned(),
        n => format!("{n} cards"),
    }
}

/// The choice row: its caption over its cards, with no controls.
///
/// A face-down card is a `.deck-back`, as a hidden hand's is, so it names no
/// card; an encounter card's is also an `.encounter-back`.
#[component]
pub fn ChoiceRowView(row: ChoiceRow) -> impl IntoView {
    let cards = match row.cards {
        RowCards::FaceDown(count, back) => (0..count)
            .map(|_| view! { <div class=back.class()></div> })
            .collect::<Vec<_>>()
            .into_any(),
        RowCards::Deck => view! { <div class="deck-back"></div> }.into_any(),
    };
    view! {
        <div class="choice-row">
            <span class="zone-label choice-row-caption">{row.caption}</span>
            <div class="choice-row-cards">{cards}</div>
        </div>
    }
}

#[cfg(test)]
mod tests;
