# Spatial board map (web) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the web board's flat locations/enemies text lists with a spatial map — positioned location-container nodes, drawn connection lines, investigators and unengaged enemies inside their location, engaged enemies + threat area in the per-investigator detail panel.

**Architecture:** All changes in `crates/web/` (Leptos 0.8 CSR). A new `map.rs` holds pure layout helpers (host-testable) and a `location_map(&GameState)` panel fn (matching the existing `locations_panel`/`enemies_panel` fn pattern — *not* a store-reading component, a deliberate simplification over the spec's wording). `board.rs` calls it and drops the two old panels; the existing `investigators_panel` gains engaged enemies + threat area. The map is a pure derivation of `GameState` (no new client state). Tests: host unit tests for the layout helpers; wasm-bindgen-test render tests (mount `BoardView`, feed a `Hello`, assert on the DOM) for the rendered map.

**Tech Stack:** Rust, Leptos 0.8 (CSR), `wasm-bindgen-test` (headless browser), plain `style.css`.

**Spec:** `docs/superpowers/specs/2026-06-27-spatial-board-map-design.md`

## Global Constraints

- **CI gauntlet (warnings-as-errors), run before pushing:** `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `RUSTFLAGS="-D warnings" cargo test --all --all-features`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `cargo build -p web --target wasm32-unknown-unknown`; `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`; `wasm-pack test --headless --firefox crates/web`.
- **Read-only board.** No interactivity — no click/hover handlers on nodes/tokens. Actions still flow through the `AwaitingInput` menu (`input.rs`), untouched.
- **Pure derivation of `GameState`.** No new signals/`ClientState` fields. Names via `crate::names::{card_name, location_name}` (registry fallback to code).
- **Layering / placement rules (from the spec):**
  - Map node holds: investigators with `current_location == this`, and enemies with `current_location == this && engaged_with.is_none()`.
  - Detail panel holds: the investigator's `threat_area` treacheries, and enemies with `engaged_with == Some(inv.id)`.
  - Only locations in `game.locations` render (set-aside locations aren't there yet).
- **Coordinates are client-side** (a code→cell table + fallback). No engine/`GameState` changes.
- **Follow the existing plain-CSS approach** (classes in `style.css`; no inline styles except the computed node `left/top/width/height`, which must be dynamic).

---

### Task 1: Layout table `location_grid_pos`

**Files:**
- Create: `crates/web/src/map.rs`
- Modify: `crates/web/src/lib.rs` (add `pub mod map;`)

**Interfaces:**
- Produces: `pub(crate) fn location_grid_pos(code: &str) -> Option<(u16, u16)>` — authored `(col, row)` grid cell for a known location code, else `None`.

- [ ] **Step 1: Create `map.rs` with the table + a failing host test**

Create `crates/web/src/map.rs`:

```rust
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
```

Add to `crates/web/src/lib.rs` after `pub mod board;`:

```rust
pub mod map;
```

- [ ] **Step 2: Run the host test to verify it passes**

Run: `cargo test -p web --lib map::`
Expected: PASS (2 tests). (These are plain host tests — `map.rs` is not wasm-gated.)

- [ ] **Step 3: Commit**

```bash
git add crates/web/src/map.rs crates/web/src/lib.rs
git commit -m "web: location-code → grid-cell layout table for the board map (#497)"
```

---

### Task 2: `layout_positions` with deterministic fallback

**Files:**
- Modify: `crates/web/src/map.rs`

**Interfaces:**
- Consumes: `location_grid_pos` (Task 1).
- Produces: `pub(crate) fn layout_positions(locations: &[(LocationId, CardCode)]) -> BTreeMap<LocationId, (u16, u16)>` — every in-play location's resolved cell (authored, or next free cell for unknown codes).

- [ ] **Step 1: Add the failing host test**

Add to `map.rs`'s `#[cfg(test)] mod tests`:

```rust
    use super::layout_positions;
    use game_core::state::{CardCode, LocationId};

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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p web --lib map::tests::authored_code_uses_its_cell_unknown_gets_a_free_one`
Expected: compile error — `layout_positions` not found.

- [ ] **Step 3: Implement `layout_positions`**

Add to `map.rs` (top-level), and the imports it needs at the top of the file:

```rust
use std::collections::{BTreeMap, BTreeSet};

use game_core::state::{CardCode, LocationId};
```

```rust
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
```

- [ ] **Step 4: Run the host tests**

Run: `cargo test -p web --lib map::`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/map.rs
git commit -m "web: resolve location grid positions with a deterministic fallback (#497)"
```

---

### Task 3: `location_map` panel — nodes + tokens, wired into the board

**Files:**
- Modify: `crates/web/src/map.rs` (add `location_map` + node rendering)
- Modify: `crates/web/src/board.rs` (call `crate::map::location_map(&game)` in the `Some(game)` arm)
- Test: `crates/web/tests/map.rs` (create)

**Interfaces:**
- Consumes: `layout_positions` (Task 2).
- Produces: `pub fn location_map(game: &GameState) -> impl IntoView` — a `<section class="map">` of absolutely-positioned `<div class="map-location" data-loc=NAME>` nodes; each node holds a `.loc-head` line plus `.inv-token` / `.enemy-token` children for the investigators and unengaged enemies in it. (Connection lines arrive in Task 4.)

- [ ] **Step 1: Add `location_map` (nodes only) to `map.rs`**

Add the Leptos imports at the top of `map.rs`:

```rust
use game_core::state::GameState;
use leptos::prelude::*;
```

Add the geometry constants and the panel fn:

```rust
/// Pixel geometry for the grid. A node occupies `NODE_W`×`NODE_H`; cells are
/// larger to leave gaps for the connection lines.
const CELL_W: u16 = 200;
const CELL_H: u16 = 150;
const NODE_W: u16 = 170;
const NODE_H: u16 = 120;

/// The map panel: one absolutely-positioned container node per in-play location,
/// holding the investigators and unengaged enemies in it. Connection lines are
/// added by [`connection_lines`] (Task 4). Read-only — pure derivation of
/// `game`.
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

    view! { <section class="map">{nodes}</section> }
}
```

- [ ] **Step 2: Wire it into `BoardView`**

In `crates/web/src/board.rs`, in the `Some(game)` arm of `board` (currently lines 30-39), insert the map call after `phase_bar`. Replace:

```rust
        Some(game) => view! {
            <div class="game">
                {resolution_banner(&game)}
                {phase_bar(&game)}
                {locations_panel(&game)}
                {investigators_panel(&game)}
                {enemies_panel(&game)}
            </div>
        }
```

with:

```rust
        Some(game) => view! {
            <div class="game">
                {resolution_banner(&game)}
                {phase_bar(&game)}
                {crate::map::location_map(&game)}
                {investigators_panel(&game)}
                {enemies_panel(&game)}
            </div>
        }
```

(`locations_panel` is now unused; Task 6 deletes it. To keep this task compiling under `-D warnings`, add `#[allow(dead_code)]` to `fn locations_panel` for now.) In `board.rs`, change `fn locations_panel` to:

```rust
#[allow(dead_code)]
fn locations_panel(game: &GameState) -> impl IntoView {
```

- [ ] **Step 3: Create the wasm render test `crates/web/tests/map.rs`**

```rust
//! Headless render tests for the spatial board map (#497). Mount `BoardView`,
//! feed a constructed `GameState`, assert on the rendered DOM. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::fixtures::{test_enemy, test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `BoardView`, feed one `Hello` carrying `state`, tick, return nothing —
/// assertions query the live DOM via `document()`.
async fn mount_state(state: game_core::state::GameState) {
    game_core::test_support::install_test_registry();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;
}

/// `textContent` of the map node whose `data-loc` equals `loc_name` (empty if no
/// such node).
fn node_text(loc_name: &str) -> String {
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    document()
        .query_selector(&sel)
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn investigator_renders_inside_its_location_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Investigator 1"),
        "investigator token must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}

#[wasm_bindgen_test]
async fn unengaged_enemy_renders_inside_its_location_node() {
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = None;
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Mock Ghoul"),
        "unengaged enemy must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}
```

(If `test_investigator(1).name` is not the string `"Investigator 1"`, read the fixture in `crates/game-core/src/test_support/fixtures.rs` and use its actual name in the assertion. Likewise confirm `test_location(id, name)` sets `name` to the passed string.)

- [ ] **Step 4: Run the host build + wasm tests**

Run: `cargo build -p web --target wasm32-unknown-unknown`
Expected: compiles.

Run: `wasm-pack test --headless --firefox crates/web -- --test map`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/map.rs crates/web/src/board.rs crates/web/tests/map.rs
git commit -m "web: render location-container nodes with investigator/enemy tokens (#497)"
```

---

### Task 4: Connection lines (SVG)

**Files:**
- Modify: `crates/web/src/map.rs` (add `connection_lines` + `map_extent`; render the SVG layer)
- Test: `crates/web/tests/map.rs` (add cases)

**Interfaces:**
- Consumes: `layout_positions`, `CELL_W/CELL_H/NODE_W/NODE_H` (Task 3).
- Produces: an `<svg class="map-lines">` layer inside the `.map` section with one `<line class="map-line">` per undirected pair of connected, in-play locations.

- [ ] **Step 1: Add `connection_lines` + `map_extent` to `map.rs`**

```rust
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
```

- [ ] **Step 2: Render the SVG layer in `location_map`**

In `location_map`, replace the final `view! { <section class="map">{nodes}</section> }` with:

```rust
    let lines = connection_lines(game, &positions);
    let (w, h) = map_extent(&positions);
    view! {
        <section class="map" style=format!("width:{w}px;height:{h}px;")>
            <svg class="map-lines" width=w height=h>{lines}</svg>
            {nodes}
        </section>
    }
}
```

- [ ] **Step 3: Add wasm tests for the lines**

Add to `crates/web/tests/map.rs`:

```rust
fn line_count() -> u32 {
    document()
        .query_selector_all("line.map-line")
        .expect("query ok")
        .length()
}

#[wasm_bindgen_test]
async fn connected_locations_draw_a_line() {
    let mut state = GameStateBuilder::new()
        .with_location(test_location(10, "Hallway"))
        .with_location(test_location(11, "Attic"))
        .with_investigator(test_investigator(1))
        .build();
    state.connect(LocationId(10), LocationId(11));
    mount_state(state).await;
    assert!(line_count() >= 1, "a connected pair must draw at least one line");
}

#[wasm_bindgen_test]
async fn isolated_location_draws_no_line() {
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study")) // no connections
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    assert_eq!(line_count(), 0, "an isolated location draws no lines");
}
```

(Confirm `GameState::connect(LocationId, LocationId)` is callable from the test — it's a `pub` method on `GameState` per `game_state.rs`. If the locations must exist first, `with_location` adds them before `connect`; both ids are present.)

- [ ] **Step 4: Run wasm tests**

Run: `wasm-pack test --headless --firefox crates/web -- --test map`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/map.rs crates/web/tests/map.rs
git commit -m "web: draw connection lines between location nodes (#497)"
```

---

### Task 5: Detail panel — threat area + engaged enemies

**Files:**
- Modify: `crates/web/src/board.rs` (`investigators_panel`)
- Test: `crates/web/tests/map.rs` (add a case)

**Interfaces:**
- Consumes: nothing new.
- Produces: each `.investigator` article also renders a `.threat` block (the investigator's `threat_area` treacheries by name) and an `.engaged` block (enemies with `engaged_with == Some(inv.id)`).

- [ ] **Step 1: Extend `investigators_panel`**

In `crates/web/src/board.rs`, inside the `.map(|inv| { ... })` closure of `investigators_panel`, after the `in_play` vec (currently ~line 116-120), add:

```rust
            let threat: Vec<_> = inv
                .threat_area
                .iter()
                .map(|c| view! { <li class="card">{crate::names::card_name(&c.code)}</li> })
                .collect();
            let engaged: Vec<_> = game
                .enemies
                .values()
                .filter(|e| e.engaged_with == Some(inv.id))
                .map(|e| {
                    view! {
                        <li class="enemy-engaged">
                            {e.name.clone()} " " {e.damage} "/" {e.max_health}
                        </li>
                    }
                })
                .collect();
```

Then, in that closure's `view! { <article class="investigator"> ... </article> }`, add two blocks after the `in-play` div (currently line 132):

```rust
                    <div class="threat"><h4>"Threat area"</h4><ul>{threat}</ul></div>
                    <div class="engaged"><h4>"Engaged enemies"</h4><ul>{engaged}</ul></div>
```

- [ ] **Step 2: Add the wasm test (engaged enemy in panel, not node)**

Add to `crates/web/tests/map.rs`:

```rust
#[wasm_bindgen_test]
async fn engaged_enemy_renders_in_detail_panel_not_in_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .with_enemy(enemy)
        .build();
    mount_state(state).await;

    // Not in the location node (engaged enemies leave the location box)…
    assert!(
        !node_text("Study").contains("Mock Ghoul"),
        "engaged enemy must NOT render in the location node; node = {:?}",
        node_text("Study"),
    );
    // …but present in the investigator detail panel.
    let panel = document()
        .query_selector(".investigators")
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default();
    assert!(
        panel.contains("Mock Ghoul"),
        "engaged enemy must render in the detail panel; panel = {panel:?}",
    );
}
```

- [ ] **Step 3: Run wasm tests + the existing board tests (the panel changed)**

Run: `wasm-pack test --headless --firefox crates/web -- --test map`
Expected: PASS (5 tests).

Run: `wasm-pack test --headless --firefox crates/web -- --test board`
Expected: PASS (the existing board tests still pass — the panel only gained blocks).

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/board.rs crates/web/tests/map.rs
git commit -m "web: show threat area + engaged enemies in the investigator panel (#497)"
```

---

### Task 6: Remove the old panels, add styles, full gauntlet

**Files:**
- Modify: `crates/web/src/board.rs` (delete `locations_panel` + `enemies_panel`; drop the now-unused `InvestigatorId` import if unused)
- Modify: `crates/web/style.css`

**Interfaces:**
- Consumes: everything above.
- Produces: the board renders the map + detail panels only; CSS positions the nodes and styles the line layer.

- [ ] **Step 1: Delete the dead panels**

In `crates/web/src/board.rs`:
- Delete `fn locations_panel(...)` (the `#[allow(dead_code)]` one) entirely.
- Delete `fn enemies_panel(...)` entirely, and remove its call `{enemies_panel(&game)}` from the `Some(game)` arm (the `.game` div should now contain `resolution_banner`, `phase_bar`, `location_map`, `investigators_panel`).
- `enemies_panel` was the only user of `InvestigatorId` in `board.rs`'s `use` (line 5). If `cargo build` then reports `InvestigatorId` unused, change the import `use game_core::state::{GameState, InvestigatorId};` to `use game_core::state::GameState;`.

The `Some(game)` arm is now:

```rust
        Some(game) => view! {
            <div class="game">
                {resolution_banner(&game)}
                {phase_bar(&game)}
                {crate::map::location_map(&game)}
                {investigators_panel(&game)}
            </div>
        }
```

- [ ] **Step 2: Add map styles to `style.css`**

Append to `crates/web/style.css`:

```css
/* Spatial board map (#497). Nodes are absolutely positioned at computed
   left/top (set inline); the SVG line layer sits behind them. */
.map { position: relative; margin: 1rem 0; }
.map-lines { position: absolute; top: 0; left: 0; z-index: 0; pointer-events: none; }
.map-line { stroke: #999; stroke-width: 2; }
.map-location { position: absolute; z-index: 1; box-sizing: border-box; border: 1px solid #333; border-radius: 4px; background: #fff; padding: 0.25rem; font-size: 0.8rem; overflow: hidden; }
.loc-head { font-weight: 600; margin-bottom: 0.15rem; }
.inv-token { color: #157a3a; }
.enemy-token { color: #a3261b; }
.engaged ul, .threat ul { list-style: none; padding-left: 0; margin: 0; }
.enemy-engaged { color: #a3261b; font-size: 0.85rem; }
```

- [ ] **Step 3: Run the full gauntlet**

Run each Global-Constraints command. Expected: all green, including:
- `cargo test -p web --lib map::` (host layout tests)
- `wasm-pack test --headless --firefox crates/web` (all web wasm tests: `board` + `map`)
- `cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings`

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/board.rs crates/web/style.css
git commit -m "web: drop the old locations/enemies text panels; style the map (#497)"
```

---

## Self-review notes

- **Spec coverage:** positioned nodes + container contents → Task 3; connection lines → Task 4; token-in-node + detail-panel split → Tasks 3 & 5; engaged-in-panel / unengaged-in-node → Tasks 3 & 5; coordinate table + fallback → Tasks 1 & 2; read-only (no handlers) → throughout; only-in-play locations → Task 3 (iterates `game.locations`); subsumes #497 → Task 5 (threat area). Non-goals (interactivity, engine hints, background image) respected.
- **Deviation from spec wording:** the map is a panel `fn location_map(&GameState)` (matching `locations_panel`/`enemies_panel`), not a standalone store-reading component — simpler, host-testable layout helpers, same render output. Noted in the header.
- **Type consistency:** `location_grid_pos(&str) -> Option<(u16,u16)>`, `layout_positions(&[(LocationId, CardCode)]) -> BTreeMap<LocationId,(u16,u16)>`, `node_center((u16,u16)) -> (u16,u16)`, `location_map(&GameState) -> impl IntoView` are used consistently across tasks. `LocationId.0` is the numeric id used for line dedup. `e.engaged_with == Some(inv.id)` matches `Enemy.engaged_with: Option<InvestigatorId>` and `Investigator.id: InvestigatorId`.
- **Verify-before-asserting:** Task 3 Step 3 flags confirming the `test_investigator`/`test_location` fixture name strings before locking the DOM assertions.
