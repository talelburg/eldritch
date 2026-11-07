//! The choice row: a choice among cards outside play, as every account that
//! doesn't answer it sees it, on the chooser's tab and with no controls.
//!
//! [`crate::prompt_layout::layout`] decides who gets the row
//! ([`CardChoice::Row`](crate::prompt_layout::CardChoice::Row)); this module
//! decides what it shows, from the choice's [`CardArea`]. A search keeps
//! its cards hidden: `glossary/Search.md`, *"that player is permitted to look
//! at all of the cards in the searched area without revealing those cards to
//! the other players."* So the row of a search carries no card code, and none
//! reaches the DOM. A discard pile is open information, so the row of a choice
//! from one (Scavenging 01073) shows the offered cards face up.
//!
//! [`ChoiceRow`] is pure and native-tested.

use game_core::engine::InputRequest;
use game_core::state::{CardCode, GameState, InvestigatorId};
use leptos::prelude::*;

use crate::card::Card;
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

/// How the row lays the area's cards out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowCards {
    /// This many cards, each face down.
    FaceDown(usize),
    /// One face-down deck, standing for the whole of one. The caption carries
    /// its count.
    Deck,
    /// These cards, face up: the cards on offer from an area that is open
    /// information.
    FaceUp(Vec<CardCode>),
}

impl ChoiceRow {
    /// The row for `request`'s choice among `area`'s cards in `game`. The
    /// caption comes from the area's kind, never the prompt's text, and the
    /// request is read only for an area whose cards are face up.
    #[must_use]
    pub fn of(area: CardArea, request: &InputRequest, game: &GameState) -> Self {
        match area {
            CardArea::SearchedTopOfDeck { count, .. } => ChoiceRow {
                caption: "being searched".to_owned(),
                cards: RowCards::FaceDown(count),
            },
            CardArea::SearchedDeck(investigator) => ChoiceRow {
                caption: format!("being searched: {}", cards(deck_size(game, investigator))),
                cards: RowCards::Deck,
            },
            CardArea::DiscardPile(_) => ChoiceRow {
                caption: "being chosen from".to_owned(),
                cards: RowCards::FaceUp(area.offered(request, game)),
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
/// card. A face-up card is the [`Card`] component, as the card modal's is.
#[component]
pub fn ChoiceRowView(row: ChoiceRow) -> impl IntoView {
    let cards = match row.cards {
        RowCards::FaceDown(count) => (0..count)
            .map(|_| view! { <div class="deck-back"></div> })
            .collect::<Vec<_>>()
            .into_any(),
        RowCards::Deck => view! { <div class="deck-back"></div> }.into_any(),
        RowCards::FaceUp(codes) => codes
            .into_iter()
            .map(|code| view! { <Card code=code/> })
            .collect::<Vec<_>>()
            .into_any(),
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
