//! Act + Agenda cards atop the board (above the map). A pure render of
//! `GameState` (mirrors `map::location_map`): the current act and agenda render
//! as cards — name + ability text from the corpus by `code`, thresholds from the
//! `Act`/`Agenda` structs. The act has no running clue counter (clues sit on
//! locations/investigators), so it shows `clues to advance: N` rather than a
//! fake `0/N`. Both cards glow + open a context menu when the live prompt anchors
//! an option to them (`OptionTarget::Act`/`Agenda`); inert otherwise.

use game_core::state::{Act, AdvanceDeck, Agenda, CardCode, GameState};
use leptos::prelude::*;

use crate::card::{parse_card_text, render_segments};

/// Which face of an act/agenda to show. During an advance the card flips from its
/// front to its reverse (the "1b" side that carries the on-advance effect) once
/// the flip is clicked (#558). `pub` because it rides in the generated component
/// props struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// The printed front (`name`/`text`).
    Front,
    /// The reverse (`back_name`/`back_text`).
    Reverse,
}

/// Which face an advancing act/agenda should show: `Reverse` once the advance has
/// passed its acknowledge (`step` ≥ `FireReverse`), else `Front`. `Front` when the
/// deck isn't advancing (#558).
fn deck_face(game: &GameState, deck: AdvanceDeck) -> Face {
    use game_core::state::{AdvanceStep, Continuation};
    for c in &game.continuations {
        if let Continuation::AdvanceReverse { deck: d, step, .. } = c {
            if *d == deck {
                return match step {
                    AdvanceStep::AwaitAck => Face::Front,
                    AdvanceStep::FireReverse | AdvanceStep::Finalize => Face::Reverse,
                };
            }
        }
    }
    Face::Front
}

/// Name (printed, or the raw code when no metadata) + rendered ability text for
/// an act/agenda card code, on the given [`Face`]. The reverse falls back to the
/// front name if a card carries no `back_name` (defensive — a real advancing
/// act/agenda always prints one).
fn name_and_text(code: &CardCode, face: Face) -> (String, Option<Vec<AnyView>>) {
    let meta = game_core::card_registry::current().and_then(|r| (r.metadata_for)(code));
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
    let name = name_src.unwrap_or_else(|| code.to_string());
    let text = text_src.map(|t| render_segments(parse_card_text(&t)));
    (name, text)
}

/// The current act as a card. Glows and opens an "Advance act" context menu when
/// the live prompt anchors an option to the act (`OptionTarget::Act`) — both the
/// open-turn Advance action and the round-end advance reaction (S5, #540). The
/// agenda has the parallel [`AgendaCard`] (#556).
// `act` is taken by value: Leptos `#[component]` generates an owned props struct.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn ActCard(act: Act, face: Face) -> impl IntoView {
    let (name, text) = name_and_text(&act.code, face);
    let threshold = act.clue_threshold;
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(&pending, game_core::OptionTarget::Act);
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
    view! {
        <article class=root_class>
            <div class="card-head">
                <span class="card-kind">"Act"</span>
                <span class="card-name">{name}</span>
            </div>
            <div class="card-text">{text}</div>
            {
                // Thresholds belong to the front face only — the reverse ("1b")
                // side prints the on-advance effect, not the advance cost (#558).
                (face == Face::Front).then(|| {
                    view! {
                        <div class="loc-stats">
                            <span>{format!("clues to advance: {threshold}")}</span>
                        </div>
                    }
                })
            }
            {
                // wasm-only trigger + menu; host build: empty, `menu_opts` consumed
                // above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}

/// The current agenda as a card. Glows and opens a "Resolve" context menu when the
/// live prompt anchors an option to the agenda (`OptionTarget::Agenda`, #556) — an
/// agenda-sourced forced effect (What's Going On?! 01105's on-advance reverse). The
/// mirror of [`ActCard`]; the doom counter lives on `GameState`, not the `Agenda`
/// struct, so it arrives as a second prop.
// `agenda` is taken by value: Leptos `#[component]` generates an owned props struct.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn AgendaCard(agenda: Agenda, doom: u8, face: Face) -> impl IntoView {
    let (name, text) = name_and_text(&agenda.code, face);
    let threshold = agenda.doom_threshold;
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = crate::interaction::options_for(&pending, game_core::OptionTarget::Agenda);
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
    view! {
        <article class=root_class>
            <div class="card-head">
                <span class="card-kind">"Agenda"</span>
                <span class="card-name">{name}</span>
            </div>
            <div class="card-text">{text}</div>
            {
                // Doom belongs to the front face only — the reverse ("1b") side
                // prints the on-advance effect, not the doom track (#558).
                (face == Face::Front).then(|| {
                    view! {
                        <div class="loc-stats">
                            <span>{format!("doom {doom}/{threshold}")}</span>
                        </div>
                    }
                })
            }
            {
                // wasm-only trigger + menu; host build: empty, `menu_opts` consumed
                // above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
            }
        </article>
    }
}

/// The current act + agenda as cards. Each is omitted when its deck is empty
/// (fixtures may carry neither).
pub fn act_agenda_view(game: &GameState) -> impl IntoView {
    let act_face = deck_face(game, AdvanceDeck::Act);
    let act = game
        .act_deck
        .get(game.act_index)
        .cloned()
        .map(|act| view! { <ActCard act=act face=act_face/> });
    let doom = game.agenda_doom;
    let agenda_face = deck_face(game, AdvanceDeck::Agenda);
    let agenda = game
        .agenda_deck
        .get(game.agenda_index)
        .cloned()
        .map(|ag| view! { <AgendaCard agenda=ag doom=doom face=agenda_face/> });
    view! { <section class="act-agenda">{act}{agenda}</section> }
}
