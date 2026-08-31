//! Spatial board map (#497): positioned location-container nodes with drawn
//! connection lines. Read-only; a pure derivation of `GameState`. The map and
//! its layout helpers live here; `board.rs` calls `location_map`.

use std::collections::{BTreeMap, BTreeSet};

use game_core::card_data::CardKind;
use game_core::state::{CardCode, GameState, Location, LocationId};
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
    // Normalize: shift so the placed nodes start at (0, 0), dropping any leading
    // empty columns/rows a departed location leaves behind (e.g. the Study's
    // column once Act 1 removes it). Interior gaps are not collapsed (no
    // Core/Dunwich layout has them).
    let min_col = out.values().map(|(c, _)| *c).min().unwrap_or(0);
    let min_row = out.values().map(|(_, r)| *r).min().unwrap_or(0);
    for (col, row) in out.values_mut() {
        *col -= min_col;
        *row -= min_row;
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

/// Pixel geometry for the grid. A node is `CARD_W` of location card plus a
/// token rail beside it, `NODE_W` wide in total; cells are larger again to
/// leave gaps for the connection lines. Node *height* is deliberately absent:
/// the node grows with its tokens rather than clipping them (#848).
const CELL_W: u16 = 400;
const CELL_H: u16 = 250;
const NODE_W: u16 = 360;
/// The card's box inside a node. Both are emitted onto the node as the CSS
/// custom properties `--loc-card-w` / `--loc-card-h`, so this is their single
/// source: `style.css` reads them rather than repeating the numbers.
///
/// `CARD_H` is a *minimum*, not a fixed height — the card grows with its text.
/// A minimum is what the connection lines need: they anchor half a `CARD_H`
/// down, and a card shorter than that (an unrevealed node is barely a header
/// tall) would leave its lines ending below it in empty space.
const CARD_W: u16 = 200;
const CARD_H: u16 = 130;

/// Anchor pixel for a node's connection lines: the middle of its *card*, not of
/// the node. The node spans card + token rail, so its own center falls in the
/// gap between the two and every line would end in empty space.
fn node_center((col, row): (u16, u16)) -> (u16, u16) {
    (col * CELL_W + CARD_W / 2, row * CELL_H + CARD_H / 2)
}

/// One numeral in a location's header: the class that gives it its shape and
/// colour, the numeral itself, and the words it carries only as a tooltip.
///
/// The header is `[shroud] Name ★victory [clues]` on a fixed three-column grid,
/// so each quantity is read by its position and its colour; the words "shroud"
/// and "clues" are gone from the face of the card and survive only in `title`.
struct Badge {
    class: &'static str,
    text: String,
    title: String,
}

impl Badge {
    /// The placeholder both slots show on an **unrevealed** location, which has
    /// neither value to print. RR glossary, "Location Cards": a location enters
    /// play unrevealed "so that the side with no shroud value and/or clue value
    /// is faceup". The engine's `Location` carries the fields regardless, so a
    /// header that just read them would print the revealed side's numbers on a
    /// face-down card.
    fn unknown(quantity: &str) -> Self {
        Self {
            class: "badge badge--unknown",
            text: "?".to_string(),
            title: format!("unrevealed: no {quantity} value"),
        }
    }

    fn shroud(loc: &Location) -> Self {
        if !loc.revealed {
            return Self::unknown("shroud");
        }
        Self {
            class: "badge badge--shroud",
            text: loc.shroud.to_string(),
            title: format!("shroud {}", loc.shroud),
        }
    }

    /// A zero clue count still renders, faded: drop the badge and "no clues
    /// here" would look exactly like "no clue slot", and the name column would
    /// shift between nodes.
    fn clues(loc: &Location) -> Self {
        if !loc.revealed {
            return Self::unknown("clue");
        }
        if loc.clues == 0 {
            return Self {
                class: "badge badge--clues is-zero",
                text: "0".to_string(),
                title: "no clues".to_string(),
            };
        }
        Self {
            class: "badge badge--clues",
            text: loc.clues.to_string(),
            title: format!("clues {}", loc.clues),
        }
    }

    fn into_view(self) -> impl IntoView {
        view! { <span class=self.class title=self.title>{self.text}</span> }
    }
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
#[allow(clippy::too_many_lines)]
pub fn location_map(game: &GameState) -> impl IntoView {
    let locs: Vec<_> = game
        .locations
        .values()
        .map(|l| (l.id, l.code.clone()))
        .collect();
    let positions = layout_positions(&locs);

    // The live prompt's options, for glow + per-node context menus (#536).
    // Absent (native / no prompt) → empty → no node is actionable.
    let pending = use_context::<crate::interaction::PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();

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
                            {e.name.clone()} " health " {e.damage} "/" {e.max_health}
                            {e.exhausted.then(|| view! { <span>" (exhausted)"</span> })}
                        </div>
                    }
                })
                .collect();
            // Cards put into play *at* the location, under nobody's control
            // (Lita Chantler 01117 in the Parlor). They are in play — RR
            // "In Play and Out of Play" counts a card "at a location" as such —
            // so the node has to show them; nobody's play area will (#771).
            let at_location: Vec<_> = loc
                .cards_at_location
                .iter()
                .map(|c| {
                    let name = crate::names::card_name(&c.code);
                    view! {
                        <div class="at-location-token">
                            {name} " (uncontrolled)"
                            {c.exhausted.then(|| view! { <span>" (exhausted)"</span> })}
                        </div>
                    }
                })
                .collect();
            let style = format!(
                "left:{left}px;top:{top}px;width:{NODE_W}px;\
                 --loc-card-w:{CARD_W}px;--loc-card-h:{CARD_H}px;"
            );
            let menu_opts = crate::interaction::options_for(
                &pending,
                game_core::OptionTarget::Location(loc.id),
            );
            let actionable = !menu_opts.is_empty();
            #[cfg(target_arch = "wasm32")]
            let open = RwSignal::new(None::<(i32, i32)>);
            let base = if loc.revealed {
                "map-location"
            } else {
                "map-location unrevealed"
            };
            let node_class = if actionable {
                format!("{base} actionable")
            } else {
                base.to_string()
            };
            // Corpus metadata (traits / ability text / victory) only for a
            // revealed location — the unrevealed side is hidden information —
            // and only when the registry knows the code (a synthetic registry
            // or an unknown code yields none).
            let meta = loc
                .revealed
                .then(|| {
                    game_core::card_registry::current().and_then(|r| (r.metadata_for)(&loc.code))
                })
                .flatten();
            let traits = meta
                .map(|m| {
                    if m.traits.is_empty() {
                        String::new()
                    } else {
                        format!("{}.", m.traits.join(". "))
                    }
                })
                .unwrap_or_default();
            let text_view = meta
                .and_then(|m| m.text.as_deref())
                .map(|t| crate::card::render_segments(crate::card::parse_card_text(t)));
            let victory_pip = meta
                .and_then(|m| match &m.kind {
                    CardKind::Location { victory, .. } => *victory,
                    _ => None,
                })
                .map(|n| {
                    view! {
                        <span class="victory-pip" title=format!("victory {n}")>
                            {format!("\u{2605}{n}")}
                        </span>
                    }
                });
            view! {
                <div class=node_class data-loc=loc.name.clone() style=style>
                    <div class="loc-card">
                        <div class="loc-head">
                            {Badge::shroud(loc).into_view()}
                            // The name is the one part of the header that may
                            // truncate, so it carries its full text as a title
                            // and the victory pip sits outside it — an ellipsis
                            // must never eat the star.
                            <span class="loc-title">
                                <span class="loc-name" title=loc.name.clone()>{loc.name.clone()}</span>
                                {victory_pip}
                            </span>
                            {Badge::clues(loc).into_view()}
                        </div>
                        <div class="card-traits">{traits}</div>
                        <div class="card-text">{text_view}</div>
                    </div>
                    // The satellite rail: one bordered box per occupant, laid
                    // out beside the card rather than inside it, so a long
                    // printed text can never push a token out of view (#848).
                    <div class="node-tokens">
                        {invs}
                        {enemies}
                        {at_location}
                    </div>
                    {
                        // wasm-only: the menu trigger + menu read/submit via web_sys /
                        // the wasm-only OutboundTx. On host the block is empty; `menu_opts`
                        // is still used above by `actionable`, so no unused-var warning.
                        #[cfg(target_arch = "wasm32")]
                        actionable.then(|| crate::interaction::menu_layer(menu_opts, open))
                    }
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

    #[test]
    fn positions_are_normalized_to_origin() {
        // Post-Study Gathering set — all authored at cols 2-3 (Study's col 0/1
        // is gone). Hallway 01112 (2,1), Attic 01113 (2,0), Cellar 01114 (3,1),
        // Parlor 01115 (2,2).
        let locs = vec![
            (LocationId(1), CardCode::new("01112")),
            (LocationId(2), CardCode::new("01113")),
            (LocationId(3), CardCode::new("01114")),
            (LocationId(4), CardCode::new("01115")),
        ];
        let pos = layout_positions(&locs);
        let min_col = pos.values().map(|(c, _)| *c).min().unwrap();
        let min_row = pos.values().map(|(_, r)| *r).min().unwrap();
        assert_eq!(min_col, 0, "leading empty column not removed: {pos:?}");
        assert_eq!(min_row, 0, "leading empty row not removed: {pos:?}");
        // Relative offset preserved: Cellar one column right of Hallway.
        assert_eq!(
            pos[&LocationId(3)].0,
            pos[&LocationId(1)].0 + 1,
            "relative column offset not preserved: {pos:?}"
        );
    }
}
