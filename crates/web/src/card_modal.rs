//! The card modal: a choice among cards **outside play** renders as the cards
//! themselves, each with a verb button, instead of as name buttons on the
//! prompt strip.
//!
//! The engine anchors each offered card by its position, and the prompt by
//! the area the choice is made among: the top 3 of a deck being searched, a
//! whole deck, or a discard pile (ADR 0011). This module reads those two
//! anchors and the cards at them from the client's own state. It never parses
//! the prompt's text or an option's label.
//!
//! The modal is the **sole** surface for the choice, as the decision modal is
//! for a decision: while it is up, the prompt strip renders neither the prompt
//! text nor these options.
//!
//! Only a search's areas and a discard pile are recognised so far. Scrying
//! 01061's looked-at cards stay on the prompt strip until their modal lands
//! (#1379).
//!
//! [`live_card_modal`] and [`CardArea`] are pure and native-tested; the view's
//! submit is wasm-only.

#[cfg(target_arch = "wasm32")]
use game_core::action::InputResponse;
use game_core::engine::{InputRequest, OptionId, OptionTarget};
use game_core::state::{CardCode, GameState, InvestigatorId};
use leptos::prelude::*;

use crate::card::Card;
#[cfg(target_arch = "wasm32")]
use crate::controls;
use crate::drag::Drag;
use crate::store::{self, ClientState};

/// The cards a card choice is made among, read from the prompt's area anchor.
///
/// The client reads each card's code from its own state, at the area's
/// positions, so the area also says how many cards there are to show face
/// down to an account that doesn't answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardArea {
    /// The top `count` cards of `investigator`'s deck, being searched (Old
    /// Book of Lore 01031, Arcane Initiate 01063). `count` is already cut to
    /// the cards the deck holds.
    SearchedTopOfDeck {
        /// The deck's owner.
        investigator: InvestigatorId,
        /// How many cards from the top.
        count: usize,
    },
    /// `investigator`'s whole deck, being searched (Research Librarian 01032).
    SearchedDeck(InvestigatorId),
    /// `investigator`'s discard pile, chosen from (Scavenging 01073). A
    /// discard pile is open information, so its cards are face up to all.
    DiscardPile(InvestigatorId),
}

/// What clicking an offered card does, as its button names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Take the searched card into hand.
    Draw,
    /// Take the chosen card from the discard pile into hand.
    Take,
}

impl Verb {
    /// The button's text.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Verb::Draw => "Draw",
            Verb::Take => "Take",
        }
    }
}

impl CardArea {
    /// The area `request` makes its choice among, when it is a card choice
    /// this module renders: a search area, and every option anchored to a card
    /// in it. `None` for any other prompt, which keeps its own surface.
    #[must_use]
    pub fn of(request: &InputRequest) -> Option<Self> {
        let area = match request.target.as_ref()? {
            OptionTarget::SearchedTopOfDeck {
                investigator,
                count,
            } => CardArea::SearchedTopOfDeck {
                investigator: *investigator,
                count: usize::from(*count),
            },
            OptionTarget::PlayerDeck(investigator) => CardArea::SearchedDeck(*investigator),
            OptionTarget::DiscardPile(investigator) => CardArea::DiscardPile(*investigator),
            _ => return None,
        };
        let every_option_in_it = !request.options.is_empty()
            && request.options.iter().all(|option| {
                option
                    .target
                    .as_ref()
                    .and_then(|t| area.position_of(t))
                    .is_some()
            });
        every_option_in_it.then_some(area)
    }

    /// Whose deck it is.
    #[must_use]
    pub fn investigator(self) -> InvestigatorId {
        match self {
            CardArea::SearchedTopOfDeck { investigator, .. }
            | CardArea::SearchedDeck(investigator)
            | CardArea::DiscardPile(investigator) => investigator,
        }
    }

    /// What an offered card's button does.
    #[must_use]
    pub fn verb(self) -> Verb {
        match self {
            CardArea::SearchedTopOfDeck { .. } | CardArea::SearchedDeck(_) => Verb::Draw,
            CardArea::DiscardPile(_) => Verb::Take,
        }
    }

    /// The area's cards in `game`, top first.
    #[must_use]
    pub fn cards(self, game: &GameState) -> Vec<CardCode> {
        self.slots(game).into_iter().map(|(_, code)| code).collect()
    }

    /// The cards `request` offers from this area in `game`, top first: what
    /// the modal shows with a verb.
    #[must_use]
    pub fn offered(self, request: &InputRequest, game: &GameState) -> Vec<CardCode> {
        self.slots(game)
            .into_iter()
            .filter(|(position, _)| option_at(request, self, *position).is_some())
            .map(|(_, code)| code)
            .collect()
    }

    /// The area's cards in `game`, top first, each with its position as an
    /// anchor names it: from the top of a deck, from the bottom of a discard
    /// pile.
    fn slots(self, game: &GameState) -> Vec<(usize, CardCode)> {
        let Some(inv) = game.investigators.get(&self.investigator()) else {
            return Vec::new();
        };
        match self {
            CardArea::SearchedTopOfDeck { count, .. } => {
                inv.deck.iter().take(count).cloned().enumerate().collect()
            }
            CardArea::SearchedDeck(_) => inv.deck.iter().cloned().enumerate().collect(),
            CardArea::DiscardPile(_) => inv.discard.iter().cloned().enumerate().rev().collect(),
        }
    }

    /// Whether the cards not offered fold away under a count. A whole deck or
    /// pile is too many to lay out; the top few are shown alongside the
    /// offered ones, so a player sees what they passed over.
    #[must_use]
    pub fn collapses_others(self) -> bool {
        matches!(self, CardArea::SearchedDeck(_) | CardArea::DiscardPile(_))
    }

    /// The position in this area of the card `anchor` names, or `None` when it
    /// names no card here.
    fn position_of(self, anchor: &OptionTarget) -> Option<usize> {
        if let (
            CardArea::DiscardPile(pile),
            OptionTarget::DiscardCard {
                investigator,
                index,
            },
        ) = (self, anchor)
        {
            return (investigator == &pile).then_some(index.to_usize());
        }
        let OptionTarget::DeckCard {
            investigator,
            index,
        } = anchor
        else {
            return None;
        };
        let index = index.to_usize();
        let within = match self {
            CardArea::SearchedTopOfDeck { count, .. } => index < count,
            CardArea::SearchedDeck(_) => true,
            CardArea::DiscardPile(_) => false,
        };
        (*investigator == self.investigator() && within).then_some(index)
    }
}

/// One card the modal shows face up, with the option that takes it when it is
/// offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalCard {
    /// The card, read from the client's state at its position.
    pub code: CardCode,
    /// The option a click on its verb button submits; `None` for a card in
    /// the area that isn't offered.
    pub option: Option<OptionId>,
}

/// The live card choice, as the answering account's modal shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardModal {
    /// The engine's prompt.
    pub prompt: String,
    /// The area the choice is made among.
    pub area: CardArea,
    /// The cards laid out in the modal, in the area's order: every offered
    /// card, and for an area that doesn't collapse, the cards passed over too.
    pub shown: Vec<ModalCard>,
    /// The area's cards not offered, folded under a count. Empty for an area
    /// that doesn't collapse, whose other cards are in `shown`.
    pub collapsed: Vec<CardCode>,
}

impl CardModal {
    /// What an offered card's button does.
    #[must_use]
    pub fn verb(&self) -> Verb {
        self.area.verb()
    }

    /// The heading over the collapsed cards, `None` when nothing is collapsed.
    #[must_use]
    pub fn collapsed_label(&self) -> Option<String> {
        let others = match self.collapsed.len() {
            0 => return None,
            1 => "1 other card".to_owned(),
            n => format!("{n} other cards"),
        };
        Some(match self.area {
            // A discard pile is chosen from, not searched.
            CardArea::DiscardPile(_) => others,
            CardArea::SearchedTopOfDeck { .. } | CardArea::SearchedDeck(_) => {
                format!("{others} searched")
            }
        })
    }
}

/// The option of `request` that takes the card at `position` in `area`, if
/// one does.
fn option_at(request: &InputRequest, area: CardArea, position: usize) -> Option<OptionId> {
    request
        .options
        .iter()
        .find(|option| option.target.as_ref().and_then(|t| area.position_of(t)) == Some(position))
        .map(|option| option.id)
}

/// The live prompt and its area, when it is a card choice this account
/// answers.
fn live_request(state: &ClientState) -> Option<(InputRequest, CardArea)> {
    let request = state.answerable_request()?;
    let area = CardArea::of(&request)?;
    Some((request, area))
}

/// The live card choice this account answers, or `None` when the live prompt
/// is not one (or there is none). Pure.
#[must_use]
pub fn live_card_modal(state: &ClientState) -> Option<CardModal> {
    let (request, area) = live_request(state)?;
    let game = state.game.as_ref()?;
    let mut shown = Vec::new();
    let mut collapsed = Vec::new();
    for (position, code) in area.slots(game) {
        match option_at(&request, area, position) {
            None if area.collapses_others() => collapsed.push(code),
            option => shown.push(ModalCard { code, option }),
        }
    }
    Some(CardModal {
        prompt: request.prompt,
        area,
        shown,
        collapsed,
    })
}

/// True iff the card modal is up.
///
/// Public because the prompt strips consult it: the modal is the sole surface
/// for its choice, so the rule lives here, with the view it governs, as
/// [`crate::decision::modal_is_live`] does for a decision. Pure.
#[must_use]
pub fn modal_is_live(state: &ClientState) -> bool {
    live_request(state).is_some()
}

/// The card modal. Renders nothing unless a card choice this account answers
/// is live.
///
/// A scrim over the board and a panel carrying the engine's prompt and the
/// cards, each rendered with the [`Card`] component, so its full text shows.
/// An offered card has its verb button under it. **Dismissible only by
/// choosing a card**: every exit submits engine input, so neither a backdrop
/// click nor Escape is wired, the rule the decision modal follows.
///
/// **Draggable** ([`crate::drag`]), so a player can move it aside and read the
/// board before choosing. A press that lands on a verb button starts no drag.
#[component]
pub fn CardModalView() -> impl IntoView {
    let store = store::use_store();
    let drag = Drag::per_prompt(move || {
        let st = store.get();
        modal_is_live(&st).then_some(st.log.len())
    });
    view! {
        {move || {
            let st = store.get();
            let Some(modal) = live_card_modal(&st) else {
                return ().into_any();
            };
            let verb = modal.verb().label();
            let collapsed_label = modal.collapsed_label();
            let shown: Vec<_> = modal
                .shown
                .into_iter()
                .map(|ModalCard { code, option }| {
                    let button = option.map(|id| {
                        view! {
                            <button
                                class="card-modal-verb"
                                on:click={
                                    #[cfg(target_arch = "wasm32")]
                                    {
                                        move |_| controls::submit(InputResponse::PickSingle(id))
                                    }
                                    #[cfg(not(target_arch = "wasm32"))]
                                    {
                                        let _ = id;
                                        |_| ()
                                    }
                                }
                            >
                                {verb}
                            </button>
                        }
                    });
                    view! {
                        <div class="card-modal-card">
                            <Card code=code/>
                            {button}
                        </div>
                    }
                })
                .collect();
            let collapsed = collapsed_label.map(|label| {
                let cards: Vec<_> = modal
                    .collapsed
                    .into_iter()
                    .map(|code| view! { <Card code=code/> })
                    .collect();
                view! {
                    <details class="card-modal-others">
                        <summary>{label}</summary>
                        <div class="card-modal-row">{cards}</div>
                    </details>
                }
            });
            view! {
                // No `on:click` on the backdrop: every exit is engine input.
                <div class="card-modal-backdrop" style=move || drag.scrim_style()></div>
                <section
                    class="card-modal"
                    style=move || drag.transform_style()
                    on:pointerdown=move |ev| drag.down(&ev)
                    on:pointermove=move |ev| drag.movement(&ev)
                    on:pointerup=move |ev| drag.up(&ev)
                >
                    <p class="card-modal-prompt">{modal.prompt}</p>
                    <div class="card-modal-row">{shown}</div>
                    {collapsed}
                </section>
            }
            .into_any()
        }}
    }
}

#[cfg(test)]
mod tests;
