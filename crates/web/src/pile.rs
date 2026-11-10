//! A pile of cards on the board, drawn the same way wherever it sits.
//!
//! An investigator's discard pile sits in the dock, and the encounter discard
//! pile and the victory display sit beside the encounter deck in the scenario
//! row. The victory display also shows its victory-point total.
//!
//! Collapsed, a pile is its label, its top card as a header tile (the name
//! only, its printed text in a hover [`Popover`], as the act and agenda show
//! theirs) and a count. The count is a control: it opens the **pile list**,
//! every card in the pile, top first, each a header tile with its text on
//! hover. The list closes on a click outside it or on Escape. It is read-only:
//! it shows the pile, which is open information, and offers nothing to do with
//! it. An empty pile reads its empty text and has nothing to open.
//!
//! The list is named "pile list" rather than "popover" because [`Popover`] is
//! already the hover text: the list is opened by a click and stays open until
//! closed, and the popovers of its cards open inside it.
//!
//! A pile takes its cards as the game stores them, bottom first, and draws
//! them top first: the top of a pile is the card placed on it last.
//!
//! A pile the engine can anchor an option on is given that anchor: an
//! investigator's discard pile is `OptionTarget::DiscardPile`, where Wendy's
//! Amulet 01014's play of the topmost event sits. While an option is anchored
//! there, the top card glows and a click on it opens the context menu, as an
//! in-play card's does. The count still opens the pile list.

use game_core::engine::OptionTarget;
use game_core::state::CardCode;
use leptos::ev::{self, MouseEvent};
use leptos::prelude::*;

use crate::card::{self, Face};
use crate::interaction;
use crate::popover::{Hover, Popover, Spot};

/// A pile: `label`, then its top card and a count that opens the pile list,
/// or `empty_text` when it holds no card. With an `anchor`, the top card
/// offers the options anchored there; with `victory_points`, a non-empty pile
/// shows its total after the count.
#[component]
pub fn Pile(
    /// The pile's cards as stored, bottom first: the top card is the last.
    cards: Vec<CardCode>,
    /// What the pile is, e.g. "Discard".
    #[prop(into)]
    label: String,
    /// What the pile reads with no card in it, e.g. "empty".
    #[prop(into)]
    empty_text: String,
    /// The anchor options on this pile carry, if the engine names one for it.
    #[prop(optional)]
    anchor: Option<OptionTarget>,
    /// The victory points the pile's cards are worth, shown as "VP N" after
    /// the count: the victory display passes its total.
    #[prop(optional)]
    victory_points: Option<u32>,
) -> impl IntoView {
    let cards: Vec<CardCode> = cards.into_iter().rev().collect();
    let body = match cards.first().cloned() {
        None => view! { <span class="pile-empty">{empty_text}</span> }.into_any(),
        Some(top) => {
            let open = RwSignal::new(None::<Spot>);
            let count = cards.len();
            let control_label = format!("{label}: {count}, show every card");
            view! {
                {top_card(&top, anchor)}
                <button
                    class="pile-count"
                    aria-label=control_label
                    on:click=move |ev: MouseEvent| {
                        ev.stop_propagation();
                        open.set(Some(Spot::of(&ev)));
                    }
                >
                    {count}
                </button>
                {victory_points.map(|vp| view! { <span class="pile-total">{format!("VP {vp}")}</span> })}
                {pile_list(cards, open)}
            }
            .into_any()
        }
    };
    view! {
        <div class="pile">
            <span class="zone-label">{label}</span>
            {body}
        </div>
    }
}

/// The pile's top card, `code`: a header tile that, while options are
/// anchored on `anchor`, glows and opens their context menu on a click.
fn top_card(code: &CardCode, anchor: Option<OptionTarget>) -> impl IntoView {
    let pending = interaction::pending_options_snapshot();
    let menu_opts = anchor
        .map(|anchor| interaction::options_for(&pending, &anchor))
        .unwrap_or_default();
    let actionable = !menu_opts.is_empty();
    let open = RwSignal::new(None::<(i32, i32)>);
    let hover = Hover::beside_menu(open);
    view! {
        <div class="pile-card pile-top" class:actionable={actionable} {..hover.handlers()}>
            {card_face(code, hover)}
            {
                // wasm-only menu trigger + menu (web_sys / OutboundTx).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
}

/// One card of a pile as a header tile: its printed name, and its printed text
/// in a [`Popover`] while the pointer is over it.
fn pile_card(code: &CardCode) -> impl IntoView {
    let hover = Hover::new();
    view! { <div class="pile-card" {..hover.handlers()}>{card_face(code, hover)}</div> }
}

/// What a header tile holds: `code`'s printed name, and its printed text in a
/// [`Popover`] that `hover` opens.
fn card_face(code: &CardCode, hover: Hover) -> impl IntoView {
    let (name, text) = card::name_and_text(code, Face::Front);
    let popover_name = name.clone();
    view! {
        <span class="pile-card-name">{name}</span>
        <Popover hover=hover>{card::card_popover(&popover_name, text.as_deref())}</Popover>
    }
}

/// The pile list: `cards`, top first, beside the spot `open` holds, over a
/// transparent backdrop that closes it on a click outside it. Escape closes it
/// too, through a window listener that lives only while the list is open, so a
/// closed list leaves nothing behind on the window.
fn pile_list(cards: Vec<CardCode>, open: RwSignal<Option<Spot>>) -> impl IntoView {
    move || {
        open.get().map(|spot| {
            let escape = window_event_listener(ev::keydown, move |ev| {
                if ev.key() == "Escape" {
                    open.set(None);
                }
            });
            on_cleanup(move || escape.remove());
            let tiles = cards.iter().map(pile_card).collect_view();
            view! {
                <div
                    class="menu-backdrop"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open.set(None);
                    }
                ></div>
                <div
                    class="pile-list"
                    class:opens-left=spot.left
                    class:opens-up=spot.up
                    style=spot.style()
                >
                    {tiles}
                </div>
            }
        })
    }
}
