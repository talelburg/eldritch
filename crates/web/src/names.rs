//! Display-name helpers for the web UI: resolve entity codes/ids to their
//! printed names, falling back to the raw code/id when unavailable. UI display
//! only — never used for engine input.

use game_core::card_registry::CardRegistry;
use game_core::state::{CardCode, GameState, LocationId};

/// Printed card name for `code` in `cards`, or the raw code when `cards` has
/// no metadata for it (an unimplemented-stub card).
pub fn card_name(cards: &CardRegistry, code: &CardCode) -> String {
    (cards.metadata_for)(code).map_or_else(|| code.to_string(), |m| m.name.clone())
}

/// Display name for a location id, or "loc {id}" when it is not in state.
pub fn location_name(game: &GameState, id: LocationId) -> String {
    game.locations
        .get(&id)
        .map_or_else(|| format!("loc {}", id.0), |l| l.name.clone())
}

#[cfg(test)]
mod tests {
    use cards::REGISTRY;
    use game_core::state::GameStateBuilder;
    use game_core::test_support::{self, TEST_REGISTRY};

    use super::*;

    #[test]
    fn card_name_returns_printed_name_with_registry() {
        assert_eq!(
            card_name(&REGISTRY, &CardCode::new("01030")),
            "Magnifying Glass"
        );
    }

    #[test]
    fn card_name_falls_back_to_code_for_unknown() {
        // Unknown code ⇒ no metadata ⇒ the raw code is shown.
        assert_eq!(card_name(&TEST_REGISTRY, &CardCode::new("99999")), "99999");
    }

    #[test]
    fn location_name_returns_state_name_then_falls_back() {
        let state = GameStateBuilder::new()
            .with_location(test_support::test_location(10, "Study"))
            .build();
        assert_eq!(location_name(&state, LocationId(10)), "Study");
        assert_eq!(location_name(&state, LocationId(99)), "loc 99");
    }
}
