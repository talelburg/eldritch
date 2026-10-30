//! Shared helpers for the card integration tests.

use cards::TEST_REGISTRIES;
use game_core::state::LocationId;
use game_core::test_support::{location, TestSession};

/// The Attic 01113: *"**Forced** - After you enter the Attic: Take 1 horror."*
const ATTIC: &str = "01113";

/// Put the Attic 01113 into play on `session`'s board, revealed and connected
/// both ways to each of `connected_to`, and return the session with the id it
/// was given.
pub(crate) fn with_attic(
    session: TestSession,
    connected_to: &[LocationId],
) -> (TestSession, LocationId) {
    let mut attic = None;
    let session = session.edit_state(|state| {
        let id = state.location_ids.mint();
        state.place(
            location(id)
                .as_card(ATTIC)
                .connections(connected_to.to_vec()),
            TEST_REGISTRIES,
        );
        // The way back: each location already on the board gains the Attic.
        for other in connected_to {
            state
                .locations
                .get_mut(other)
                .expect("a connected location is in play")
                .connections
                .push(id);
        }
        attic = Some(id);
    });
    (session, attic.expect("minted"))
}
