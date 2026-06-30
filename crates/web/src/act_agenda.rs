//! Act + Agenda cards atop the board (above the map). A pure render of
//! `GameState` (mirrors `map::location_map`): the current act and agenda render
//! as cards — name + ability text from the corpus by `code`, thresholds from the
//! `Act`/`Agenda` structs. The act has no running clue counter (clues sit on
//! locations/investigators), so it shows `clues to advance: N` rather than a
//! fake `0/N`. Display-only.

use game_core::state::{CardCode, GameState};
use leptos::prelude::*;

use crate::card::{parse_card_text, render_segments};

/// Name (printed, or the raw code when no metadata) + rendered ability text for
/// an act/agenda card code.
fn name_and_text(code: &CardCode) -> (String, Option<Vec<AnyView>>) {
    let meta = game_core::card_registry::current().and_then(|r| (r.metadata_for)(code));
    let name = meta.map_or_else(|| code.to_string(), |m| m.name.clone());
    let text = meta
        .and_then(|m| m.text.as_deref())
        .map(|t| render_segments(parse_card_text(t)));
    (name, text)
}

/// The current act + agenda as cards. Each is omitted when its deck is empty
/// (fixtures may carry neither).
pub fn act_agenda_view(game: &GameState) -> impl IntoView {
    let act = game.act_deck.get(game.act_index).map(|act| {
        let (name, text) = name_and_text(&act.code);
        let threshold = act.clue_threshold;
        view! {
            <article class="card card--act">
                <div class="card-head">
                    <span class="card-kind">"Act"</span>
                    <span class="card-name">{name}</span>
                </div>
                <div class="card-text">{text}</div>
                <div class="loc-stats">
                    <span>{format!("clues to advance: {threshold}")}</span>
                </div>
            </article>
        }
    });
    let agenda = game.agenda_deck.get(game.agenda_index).map(|ag| {
        let (name, text) = name_and_text(&ag.code);
        let doom = game.agenda_doom;
        let threshold = ag.doom_threshold;
        view! {
            <article class="card card--agenda">
                <div class="card-head">
                    <span class="card-kind">"Agenda"</span>
                    <span class="card-name">{name}</span>
                </div>
                <div class="card-text">{text}</div>
                <div class="loc-stats">
                    <span>{format!("doom {doom}/{threshold}")}</span>
                </div>
            </article>
        }
    });
    view! { <section class="act-agenda">{act}{agenda}</section> }
}
