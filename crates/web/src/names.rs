//! Display-name helpers for the web UI (#484): resolve entity codes/ids to their
//! printed names, falling back to the raw code/id when unavailable. UI display
//! only — never used for engine input.

use game_core::state::{CardCode, GameState, LocationId};

/// Printed card name for `code`, or the raw code when the name is unavailable —
/// an unimplemented-stub card (no metadata) or the card registry not installed
/// (e.g. a headless/native render path). The registry is installed by the web
/// binary at startup (`main.rs`).
pub fn card_name(code: &CardCode) -> String {
    game_core::card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map_or_else(|| code.to_string(), |m| m.name.clone())
}

/// Display name for a location id, or "loc {id}" when it is not in state.
pub fn location_name(game: &GameState, id: LocationId) -> String {
    game.locations
        .get(&id)
        .map_or_else(|| format!("loc {}", id.0), |l| l.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::state::GameStateBuilder;
    use game_core::test_support::fixtures::test_location;

    #[test]
    fn card_name_returns_printed_name_with_registry() {
        // The web crate depends on `cards`; installing its registry is idempotent
        // (OnceLock, first-wins) and safe in the web lib test binary, which has no
        // competing installer.
        let _ = game_core::card_registry::install(cards::REGISTRY);
        assert_eq!(card_name(&CardCode::new("01030")), "Magnifying Glass");
    }

    #[test]
    fn card_name_falls_back_to_code_for_unknown() {
        // Unknown code ⇒ no metadata ⇒ the raw code is shown (registry-agnostic).
        assert_eq!(card_name(&CardCode::new("99999")), "99999");
    }

    #[test]
    fn location_name_returns_state_name_then_falls_back() {
        let state = GameStateBuilder::new()
            .with_location(test_location(10, "Study"))
            .build();
        assert_eq!(location_name(&state, LocationId(10)), "Study");
        assert_eq!(location_name(&state, LocationId(99)), "loc 99");
    }
}
