//! Location reveal-on-entry (Rules Reference p.14): the first time an
//! investigator enters a location it is revealed and clues are placed
//! (`PerInvestigator(n) → n × #investigators`, or `Fixed(n)`). Enemy
//! movement does not reveal — only the investigator-entry call sites
//! (seating, `move_action`, and act-1's board-build native effect) call
//! this.

use crate::card_data::ClueValue;
use crate::event::Event;
use crate::state::LocationId;

use super::Cx;

/// Reveal `location_id` if it is unrevealed, placing its printed clues.
/// No-op if the location is absent or already revealed. Public so
/// card-local [`Effect::Native`](crate::dsl::Effect::Native) handlers can
/// reveal a location they move investigators into.
pub fn reveal_location(cx: &mut Cx, location_id: LocationId) {
    // "Number of investigators who started the scenario" — `len()` is
    // faithful because eliminated investigators stay in the map (status
    // flipped, never removed). If that invariant changes, per-investigator
    // math here must switch to a stored started-count.
    let count = u8::try_from(cx.state.investigators.len()).unwrap_or(u8::MAX);
    let Some(loc) = cx.state.locations.get_mut(&location_id) else {
        return;
    };
    if loc.revealed {
        return;
    }
    loc.revealed = true;
    let clues = match loc.printed_clues {
        ClueValue::PerInvestigator(n) => n.saturating_mul(count),
        ClueValue::Fixed(n) => n,
    };
    loc.clues = clues;
    cx.events.push(Event::LocationRevealed {
        location: location_id,
        clues,
    });
}

#[cfg(test)]
mod tests {
    use super::reveal_location;
    use crate::card_data::ClueValue;
    use crate::engine::Cx;
    use crate::event::Event;
    use crate::state::{CardCode, Location, LocationId};
    use crate::test_support::{test_investigator, GameStateBuilder};

    fn unrevealed(id: u32, code: &str, printed: ClueValue) -> Location {
        let mut loc = Location::new(LocationId(id), CardCode(code.into()), "L", 1, 0);
        loc.revealed = false;
        loc.printed_clues = printed;
        loc.clues = 0;
        loc
    }

    #[test]
    fn reveal_places_per_investigator_clues_times_count() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(unrevealed(5, "x", ClueValue::PerInvestigator(2)))
            .build();
        let mut events = Vec::new();
        reveal_location(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            LocationId(5),
        );
        let loc = &state.locations[&LocationId(5)];
        assert!(loc.revealed);
        assert_eq!(loc.clues, 4, "2 per-investigator × 2 investigators");
        assert!(events.iter().any(|e| matches!(e, Event::LocationRevealed { location, clues } if *location == LocationId(5) && *clues == 4)));
    }

    #[test]
    fn reveal_places_fixed_clues_regardless_of_count() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(unrevealed(5, "x", ClueValue::Fixed(3)))
            .build();
        let mut events = Vec::new();
        reveal_location(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            LocationId(5),
        );
        assert_eq!(state.locations[&LocationId(5)].clues, 3);
    }

    #[test]
    fn reveal_is_idempotent_on_already_revealed() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(Location::new(
                LocationId(5),
                CardCode("x".into()),
                "L",
                1,
                9,
            ))
            .build();
        let mut events = Vec::new();
        reveal_location(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            LocationId(5),
        );
        assert_eq!(state.locations[&LocationId(5)].clues, 9, "unchanged");
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::LocationRevealed { .. })));
    }
}
