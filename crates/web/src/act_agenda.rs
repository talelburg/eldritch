//! The scenario row atop the board (above the map): the victory display and
//! the encounter discard pile, drawn as [`Pile`]s, then the encounter deck,
//! the agenda and the act.
//!
//! A pure render of `GameState` (mirrors `map::location_map`): the current act
//! and agenda render as header-only cards — the name from the corpus by `code`,
//! thresholds from the `Act`/`Agenda` structs — and a hover [`Popover`] shows
//! the face's printed text. The act has no running clue
//! counter (clues sit on locations/investigators), so it shows `clues to
//! advance: N` rather than a fake `0/N` ([`clue_line`]). Both cards glow + open a context menu
//! when the live prompt anchors an option to them
//! (`OptionTarget::Act`/`Agenda`); inert otherwise.

use card_dsl::card_data::{CardKind, ClueValue};
use game_core::card_registry::CardRegistry;
use game_core::engine::OptionTarget;
use game_core::state::{
    Act, AdvanceDeck, AdvanceReverseFrame, AdvanceStep, Agenda, CardCode, Frame, GameState,
};
use leptos::prelude::*;

use crate::interaction::{self, PendingOptions};
use crate::pile::Pile;
use crate::popover::{Hover, Popover};
use crate::{card, controls, registry};

/// Which face of an act/agenda to show.
///
/// During an advance the card flips from its front to its reverse (the "1b"
/// side that carries the on-advance effect) once the flip is clicked. `pub`
/// because it rides in the generated component props struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// The printed front (`name`/`text`).
    Front,
    /// The reverse (`back_name`/`back_text`).
    Reverse,
}

/// Which face an advancing act/agenda should show: `Reverse` once the advance has
/// passed its acknowledge (`step` ≥ `FireReverse`), else `Front`. `Front` when the
/// deck isn't advancing.
pub(crate) fn deck_face(game: &GameState, deck: AdvanceDeck) -> Face {
    // A scan rather than `topmost_of`, which can't filter on the deck: the
    // topmost advance may be the other deck's.
    let advancing = game
        .continuations
        .iter()
        .filter_map(AdvanceReverseFrame::downcast_ref)
        .find(|frame| frame.deck == deck);
    match advancing.map(|frame| frame.step) {
        None | Some(AdvanceStep::Announce | AdvanceStep::AwaitAck) => Face::Front,
        Some(AdvanceStep::FireReverse | AdvanceStep::Finalize) => Face::Reverse,
    }
}

/// Name (printed, or the raw code when no metadata) + raw ability text for an
/// act/agenda card code, on the given [`Face`]. The reverse falls back to the
/// front name if a card carries no `back_name` (defensive — a real advancing
/// act/agenda always prints one).
///
/// `pub(crate)` because the decision modal names the card it is printed on from the very
/// same face the board is showing (ADR 0015) — the front/reverse choice is
/// computed once, here, rather than re-derived per surface.
pub(crate) fn name_and_text_src(
    cards: &CardRegistry,
    code: &CardCode,
    face: Face,
) -> (String, Option<String>) {
    let meta = (cards.metadata_for)(code);
    let (name_src, text_src) = match face {
        Face::Front => (
            meta.map(|m| m.name.clone()),
            meta.and_then(|m| m.text.clone()),
        ),
        Face::Reverse => (
            meta.and_then(|m| m.back_name.clone())
                .or_else(|| meta.map(|m| m.name.clone())),
            meta.and_then(|m| m.back_text.clone()),
        ),
    };
    (name_src.unwrap_or_else(|| code.to_string()), text_src)
}

/// [`name_and_text_src`] read from the registry provided at mount.
fn name_and_text(code: &CardCode, face: Face) -> (String, Option<String>) {
    name_and_text_src(registry::use_cards(), code, face)
}

/// The act's front-face clue line, `None` for an act that advances on a
/// non-clue objective.
///
/// A per-investigator value reads `clues to advance: 2 per investigator (4)`,
/// the effective total for `game`'s investigators in parentheses; a fixed one
/// reads `clues to advance: 2`.
#[must_use]
pub fn clue_line(act: &Act, game: &GameState) -> Option<String> {
    let total = act.clues_to_advance(game)?;
    Some(match act.printed_clues? {
        ClueValue::PerInvestigator(n) => {
            format!("clues to advance: {n} per investigator ({total})")
        }
        ClueValue::Fixed(n) => format!("clues to advance: {n}"),
    })
}

/// The current act as a card. Glows and opens an "Advance act" context menu when
/// the live prompt anchors an option to the act (`OptionTarget::Act`) — both the
/// open-turn Advance action and the round-end advance reaction. The
/// agenda has the parallel [`AgendaCard`]. Its front-face `clue_text` arrives
/// as a prop, [`clue_line`] computed by the caller, since the total reads the
/// investigator count from `GameState`, not the `Act`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Leptos `#[component]` takes `act` by value into its owned props struct"
)]
#[component]
pub fn ActCard(act: Act, clue_text: Option<String>, face: Face) -> impl IntoView {
    let (name, text) = name_and_text(&act.code, face);
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = interaction::options_for(&pending, &OptionTarget::Act);
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    let mut root_class = String::from("card card--act");
    if face == Face::Reverse {
        root_class.push_str(" card--reverse");
    }
    if actionable {
        root_class.push_str(" actionable");
    }
    let hover = Hover::new();
    let popover_name = name.clone();
    view! {
        <article
            class={root_class}
            {..hover.handlers()}
        >
            <div class="card-head">
                <span class="card-kind">"Act"</span>
                <span class="card-name">{name}</span>
            </div>
            <Popover hover=hover>{card::card_popover(&popover_name, text.as_deref())}</Popover>
            {
                // Thresholds belong to the front face only — the reverse ("1b")
                // side prints the on-advance effect, not the advance cost.
                clue_text.filter(|_| face == Face::Front).map(|line| {
                    view! {
                        <div class="threshold-stats">
                            <span>{line}</span>
                        </div>
                    }
                })
            }
            {
                // wasm-only trigger + menu; host build: empty, `menu_opts` consumed
                // above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}

/// The current agenda as a card. Glows and opens a "Resolve" context menu when the
/// live prompt anchors an option to the agenda (`OptionTarget::Agenda`) — an
/// agenda-sourced forced effect (What's Going On?! 01105's on-advance reverse). The
/// mirror of [`ActCard`]; the doom counter lives on `GameState`, not the `Agenda`
/// struct, so it arrives as a second prop.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Leptos `#[component]` takes `agenda` by value into its owned props struct"
)]
#[component]
pub fn AgendaCard(agenda: Agenda, doom: u8, face: Face) -> impl IntoView {
    let (name, text) = name_and_text(&agenda.code, face);
    let threshold = agenda.doom_threshold;
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = interaction::options_for(&pending, &OptionTarget::Agenda);
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    let mut root_class = String::from("card card--agenda");
    if face == Face::Reverse {
        root_class.push_str(" card--reverse");
    }
    if actionable {
        root_class.push_str(" actionable");
    }
    let hover = Hover::new();
    let popover_name = name.clone();
    view! {
        <article
            class={root_class}
            {..hover.handlers()}
        >
            <div class="card-head">
                <span class="card-kind">"Agenda"</span>
                <span class="card-name">{name}</span>
            </div>
            <Popover hover=hover>{card::card_popover(&popover_name, text.as_deref())}</Popover>
            {
                // Doom belongs to the front face only — the reverse ("1b") side
                // prints the on-advance effect, not the doom track.
                (face == Face::Front).then(|| {
                    view! {
                        <div class="threshold-stats">
                            <span>{format!("doom {doom}/{threshold}")}</span>
                        </div>
                    }
                })
            }
            {
                // wasm-only trigger + menu; host build: empty, `menu_opts` consumed
                // above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}

/// The scenario row atop the board.
///
/// Left to right: the victory display (with its total), the encounter discard
/// pile, the encounter deck, and the current agenda and act as cards. The act
/// and agenda are each omitted when their deck is empty (fixtures may carry
/// neither); the piles always keep their place.
pub fn act_agenda_view(game: &GameState) -> impl IntoView {
    let act_face = deck_face(game, AdvanceDeck::Act);
    let act = game.act_deck.get(game.act_index).cloned().map(|act| {
        let clue_text = clue_line(&act, game);
        view! { <ActCard act=act clue_text=clue_text face=act_face/> }
    });
    let doom = game.agenda_doom;
    let agenda_face = deck_face(game, AdvanceDeck::Agenda);
    let agenda = game
        .agenda_deck
        .get(game.agenda_index)
        .cloned()
        .map(|ag| view! { <AgendaCard agenda=ag doom=doom face=agenda_face/> });
    // The encounter deck sits beside the act and agenda: it is scenario-level,
    // not per-investigator, and the Mythos draw is an act on the board rather
    // than a "Confirm" with no referent.
    let encounter = controls::encounter_deck_view(game);
    // The scenario's out-of-play areas sit together on the left, its progress
    // on the right. Both piles are stored bottom first, so the top is last.
    let encounter_discard: Vec<CardCode> = game.encounter_discard.iter().rev().cloned().collect();
    let victory_display: Vec<CardCode> = game.victory_display.iter().rev().cloned().collect();
    let victory_points = victory_total(registry::use_cards(), &victory_display);
    view! {
        <section class="act-agenda">
            <Pile
                cards=victory_display
                label="Victory display"
                empty_text="—"
                victory_points=victory_points
            />
            <Pile cards=encounter_discard label="Encounter discard" empty_text="empty"/>
            {encounter}
            {agenda}
            {act}
        </section>
    }
}

/// The victory points of the cards in a victory display: the sum of each
/// card's printed Victory X. An enemy, a location and an act can print one; any
/// other card, and a card with no metadata, counts 0.
#[must_use]
pub fn victory_total(cards: &CardRegistry, codes: &[CardCode]) -> u32 {
    codes
        .iter()
        .filter_map(|code| (cards.metadata_for)(code))
        .filter_map(|meta| match &meta.kind {
            CardKind::Enemy { victory, .. }
            | CardKind::Location { victory, .. }
            | CardKind::Act { victory, .. } => *victory,
            _ => None,
        })
        .map(u32::from)
        .sum()
}

#[cfg(test)]
mod tests {
    use cards::TEST_REGISTRY;
    use game_core::state::CardCode;

    use super::victory_total;

    #[test]
    fn the_ghoul_priest_is_worth_two_victory_points() {
        // Ghoul Priest 01116 prints Victory 2.
        assert_eq!(victory_total(&TEST_REGISTRY, &[CardCode::new("01116")]), 2);
    }

    #[test]
    fn a_card_with_no_metadata_is_worth_nothing() {
        assert_eq!(victory_total(&TEST_REGISTRY, &[CardCode::new("_none")]), 0);
    }
}
