//! Placing an investigator at a location: a relocation that is not a move.
//!
//! A card that *places* an investigator (Trapped 01108's *"Place each
//! investigator in the Hallway"*) relocates them without performing a move, so
//! nothing a move carries happens here: no engaged enemy comes along, and no
//! move trigger fires. The Move action's own relocation lives in
//! `actions/move.rs`.

use crate::engine::Cx;
use crate::event::Event;
use crate::state::{InvestigatorId, LocationId};

/// Place `investigator` at `location`. Re-exported at
/// `game_core::engine::place_investigator` for card-local natives.
///
/// Emits [`Event::InvestigatorMoved`] only when the investigator was at a
/// location and it differs from `location`. The event's `from` is not
/// optional, so placing an investigator who had no location sets it
/// silently, and placing one where they already are announces nothing.
///
/// No-op for an investigator not in the game.
pub fn place_investigator(cx: &mut Cx, investigator: InvestigatorId, location: LocationId) {
    let Some(inv) = cx.state.investigators.get_mut(&investigator) else {
        return;
    };
    let from = inv.current_location.replace(location);
    if let Some(from) = from.filter(|&from| from != location) {
        cx.events.push(Event::InvestigatorMoved {
            investigator,
            from,
            to: location,
        });
    }
}
