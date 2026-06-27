//! Spatial board map (#497): positioned location-container nodes with drawn
//! connection lines. Read-only; a pure derivation of `GameState`. The map and
//! its layout helpers live here; `board.rs` calls `location_map`.

use std::collections::{BTreeMap, BTreeSet};

use game_core::state::{CardCode, GameState, LocationId};
use leptos::prelude::*;

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

/// Number of columns the fallback flows across before wrapping to a new row.
/// Generous — the fallback is a degraded path for scenarios without an authored
/// layout; authored cells stay well within this.
const FALLBACK_COLS: u16 = 6;

/// Resolve a `(col, row)` grid cell for every in-play location: its authored
/// cell from [`location_grid_pos`], or — for codes without one — the next free
/// cell in row-major order, skipping cells already taken (authored or
/// previously assigned). Deterministic in `locations` order, so the layout is
/// stable across renders.
pub(crate) fn layout_positions(
    locations: &[(LocationId, CardCode)],
) -> BTreeMap<LocationId, (u16, u16)> {
    // All authored cells are reserved up front so a fallback never lands on one.
    let mut taken: BTreeSet<(u16, u16)> = locations
        .iter()
        .filter_map(|(_, code)| location_grid_pos(code.as_str()))
        .collect();
    let mut cursor: (u16, u16) = (0, 0);
    let mut out = BTreeMap::new();
    for (id, code) in locations {
        let pos = location_grid_pos(code.as_str()).unwrap_or_else(|| {
            while taken.contains(&cursor) {
                cursor = advance_cell(cursor);
            }
            let p = cursor;
            taken.insert(p);
            cursor = advance_cell(cursor);
            p
        });
        out.insert(*id, pos);
    }
    out
}

/// Row-major next cell, wrapping after [`FALLBACK_COLS`] columns.
fn advance_cell((col, row): (u16, u16)) -> (u16, u16) {
    if col + 1 >= FALLBACK_COLS {
        (0, row + 1)
    } else {
        (col + 1, row)
    }
}

/// Pixel geometry for the grid. A node occupies `NODE_W`×`NODE_H`; cells are
/// larger to leave gaps for the connection lines.
const CELL_W: u16 = 200;
const CELL_H: u16 = 150;
const NODE_W: u16 = 170;
const NODE_H: u16 = 120;

/// Center pixel of a node at grid cell `(col, row)`.
fn node_center((col, row): (u16, u16)) -> (u16, u16) {
    (col * CELL_W + NODE_W / 2, row * CELL_H + NODE_H / 2)
}

/// One `<line>` per undirected pair of connected, in-play locations, between
/// node centers. A peer not in `positions` (set-aside, not yet in play) is
/// skipped. Dedups by ordered `LocationId` pair so each edge draws once.
fn connection_lines(
    game: &GameState,
    positions: &BTreeMap<LocationId, (u16, u16)>,
) -> Vec<impl IntoView> {
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut lines = Vec::new();
    for loc in game.locations.values() {
        let Some(&a) = positions.get(&loc.id) else {
            continue;
        };
        for peer in &loc.connections {
            let Some(&b) = positions.get(peer) else {
                continue; // peer not in play
            };
            let key = (loc.id.0.min(peer.0), loc.id.0.max(peer.0));
            if !seen.insert(key) {
                continue;
            }
            let (x1, y1) = node_center(a);
            let (x2, y2) = node_center(b);
            lines.push(view! {
                <line class="map-line" x1=x1 y1=y1 x2=x2 y2=y2 />
            });
        }
    }
    lines
}

/// Pixel `(width, height)` spanning all placed nodes (one extra cell of slack).
fn map_extent(positions: &BTreeMap<LocationId, (u16, u16)>) -> (u16, u16) {
    let max_col = positions.values().map(|(c, _)| *c).max().unwrap_or(0);
    let max_row = positions.values().map(|(_, r)| *r).max().unwrap_or(0);
    ((max_col + 1) * CELL_W, (max_row + 1) * CELL_H)
}

/// The map panel: one absolutely-positioned container node per in-play location,
/// holding the investigators and unengaged enemies in it. Connection lines are
/// drawn by a private helper; SVG lines sit behind the nodes. Read-only —
/// pure derivation of `game`.
pub fn location_map(game: &GameState) -> impl IntoView {
    let locs: Vec<_> = game
        .locations
        .values()
        .map(|l| (l.id, l.code.clone()))
        .collect();
    let positions = layout_positions(&locs);

    let nodes: Vec<_> = game
        .locations
        .values()
        .map(|loc| {
            let (col, row) = positions[&loc.id];
            let (left, top) = (col * CELL_W, row * CELL_H);
            let invs: Vec<_> = game
                .investigators
                .values()
                .filter(|i| i.current_location == Some(loc.id))
                .map(|i| {
                    view! {
                        <div class="inv-token">
                            {i.name.clone()} " " {i.damage()} "/" {i.max_health()} " hp · "
                            {i.horror()} "/" {i.max_sanity()} " san · clues " {i.clues}
                        </div>
                    }
                })
                .collect();
            let enemies: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.current_location == Some(loc.id) && e.engaged_with.is_none())
                .map(|e| {
                    view! {
                        <div class="enemy-token">
                            {e.name.clone()} " " {e.damage} "/" {e.max_health}
                        </div>
                    }
                })
                .collect();
            let style = format!("left:{left}px;top:{top}px;width:{NODE_W}px;height:{NODE_H}px;");
            view! {
                <div class="map-location" data-loc=loc.name.clone() style=style>
                    <div class="loc-head">
                        {loc.name.clone()} " (shroud " {loc.shroud} " · clues " {loc.clues} ")"
                    </div>
                    {invs}
                    {enemies}
                </div>
            }
        })
        .collect();

    let lines = connection_lines(game, &positions);
    let (w, h) = map_extent(&positions);
    view! {
        <section class="map" style=format!("width:{w}px;height:{h}px;")>
            <svg class="map-lines" width=w height=h>{lines}</svg>
            {nodes}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::{layout_positions, location_grid_pos};
    use game_core::state::{CardCode, LocationId};

    #[test]
    fn known_gathering_codes_have_authored_cells() {
        assert_eq!(location_grid_pos("01112"), Some((2, 1)));
        assert_eq!(location_grid_pos("01113"), Some((2, 0)));
        assert_eq!(location_grid_pos("01111"), Some((0, 1)));
        assert_eq!(location_grid_pos("01114"), Some((3, 1))); // Cellar
        assert_eq!(location_grid_pos("01115"), Some((2, 2))); // Parlor
    }

    #[test]
    fn unknown_code_has_no_authored_cell() {
        assert_eq!(location_grid_pos("99999"), None);
    }

    #[test]
    fn authored_code_uses_its_cell_unknown_gets_a_free_one() {
        let locs = vec![
            (LocationId(1), CardCode::new("01112")), // authored (2, 1)
            (LocationId(2), CardCode::new("99999")), // fallback
        ];
        let pos = layout_positions(&locs);
        assert_eq!(pos[&LocationId(1)], (2, 1));
        // The fallback location gets *some* cell, and it must not collide with
        // the authored cell.
        assert_ne!(pos[&LocationId(2)], (2, 1));
    }

    #[test]
    fn two_unknown_codes_get_distinct_cells() {
        let locs = vec![
            (LocationId(1), CardCode::new("aaaaa")),
            (LocationId(2), CardCode::new("bbbbb")),
        ];
        let pos = layout_positions(&locs);
        assert_ne!(pos[&LocationId(1)], pos[&LocationId(2)]);
    }
}
