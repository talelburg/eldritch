//! Spatial board map (#497): positioned location-container nodes with drawn
//! connection lines. Read-only; a pure derivation of `GameState`. The map and
//! its layout helpers live here; `board.rs` calls `location_map`.

/// Authored grid cell `(col, row)` for a known location code — the layout the
/// client ships for scenarios it knows. The Gathering: the Study sits isolated
/// to the left; the Hallway is the hub, with the Attic above, the Parlor below,
/// and the Cellar to its right. Codes without an authored cell return `None` and
/// are placed by the fallback in [`layout_positions`].
pub(crate) fn location_grid_pos(code: &str) -> Option<(u16, u16)> {
    match code {
        "01111" => Some((0, 1)), // Study (isolated)
        "01112" => Some((2, 1)), // Hallway (hub)
        "01113" => Some((2, 0)), // Attic
        "01114" => Some((3, 1)), // Cellar
        "01115" => Some((2, 2)), // Parlor
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::location_grid_pos;

    #[test]
    fn known_gathering_codes_have_authored_cells() {
        assert_eq!(location_grid_pos("01112"), Some((2, 1)));
        assert_eq!(location_grid_pos("01113"), Some((2, 0)));
        assert_eq!(location_grid_pos("01111"), Some((0, 1)));
    }

    #[test]
    fn unknown_code_has_no_authored_cell() {
        assert_eq!(location_grid_pos("99999"), None);
    }
}
