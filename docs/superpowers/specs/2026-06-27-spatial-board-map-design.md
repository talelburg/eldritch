# Spatial board map (web) — design (#497, expanded)

## Problem

The web board (`crates/web/src/board.rs`) renders three flat text panels — a
locations list, an investigators list, and an enemies list. Nothing shows
*where* things are: a player can't see the location graph, which location they're
in, where enemies lurk, or (the original #497 complaint) what's in their threat
area. Found repeatedly in live playtest.

## Goal

A spatial board: a map of the in-play locations with their connections drawn as
lines, each location a container holding the investigators and unengaged enemies
that are *in* it, plus a per-investigator detail panel for the state that doesn't
fit on the map (hand, assets, threat area, engaged enemies). Read-only — actions
still flow through the existing `AwaitingInput` menu.

This **subsumes #497** (threat-area display is part of the detail panel).

## Decisions from brainstorming

- **Positioned map with drawn connection lines** *and* **location-as-container** —
  the nodes on the map are rich boxes, with SVG lines between connected boxes.
- **Token-in-node + detail panel** — the location box shows a compact investigator
  token; full investigator detail lives in a separate panel.
- **Engaged enemies render with the investigator** (in the detail panel), not in
  the location box; unengaged enemies render in the location box.
- **Client-side coordinate table** keyed by location code, with a graceful
  fallback for codes not in the table. Engine-provided layout hints are a bigger
  cross-crate change, deferred.
- **Read-only**; no click/hover interaction, no background image, no drag.

## Architecture

All changes are in `crates/web/` (Leptos 0.8 CSR). The full `GameState` is already
available to views via the store (`use_store().get().game`), so the map is a pure
derivation of state — no new client state.

### Components

- **`LocationMap`** (new component; new file `crates/web/src/map.rs`, exported from
  `lib.rs`). Reads the game from the store and renders the positioned location
  nodes + an SVG connection-line layer. Replaces today's `locations_panel` and
  `enemies_panel` in `board.rs`.
- **Location node** — a container box per in-play location showing:
  - location name (`location.name`), `shroud`, `clues`;
  - **investigator tokens** for each investigator whose `current_location` is this
    location — compact: name + `health`/`max_health`, `sanity`/`max_sanity` (i.e.
    `damage`/`horror` against capacity), and carried `clues`;
  - **unengaged enemy tokens** for each enemy where `current_location == this` and
    `engaged_with.is_none()` — name + `damage`/`max_health`.
- **Investigator detail panel** — the existing `investigators_panel` in `board.rs`,
  kept and extended. Per investigator: `resources`, `actions_remaining`,
  `health`/`sanity` (damage/horror vs capacity), `clues`, `status`, hand
  (`names::card_name`), in-play assets, threat-area treacheries (`threat_area`),
  **and** the investigator's **engaged enemies** (`Enemy` entities where
  `engaged_with == Some(this.id)`).

### Layout & connection lines

- A pure function `location_grid_pos(code: &str) -> Option<(u16, u16)>` returns a
  `(col, row)` grid cell for known location codes. The Gathering's five locations:
  Study isolated to one side; Hallway as the hub with Attic / Cellar / Parlor on
  its three sides. Exact cells are an implementation detail of the table.
- Nodes render at `(col, row)` on a fixed-cell grid (each cell a fixed size, so
  centers are computable as `col * CELL_W + CELL_W/2`, etc.).
- **Fallback:** in-play locations whose code is absent from the table are assigned
  the next free grid cells deterministically (e.g. appended in `game.locations`
  iteration order), so future scenarios render — unstyled-but-functional — until
  their coordinates are added.
- **Lines:** an SVG layer positioned behind the nodes draws one `<line>` per pair
  of connected, in-play locations (`location.connections`, dedup undirected pairs),
  endpoints at the two node centers. Set-aside locations aren't in `game.locations`
  yet, so they neither render nor get lines until they enter play.

### Data flow

`LocationMap` reads `store.get().game`, then derives (all by iterating
`GameState`):
- investigators grouped by `current_location`;
- enemies with `engaged_with.is_none()` grouped by `current_location` (unengaged →
  location box);
- enemies with `engaged_with == Some(id)` grouped by investigator (→ detail panel).

No mutation, no new signals. Re-renders reactively when the store's `game` changes
(same pattern as the current panels).

### Styling

Extend `crates/web/style.css`:
- `.map` — `position: relative`, sized to the grid extent.
- `.map-lines` — an absolutely-positioned SVG layer behind the nodes (`z-index`
  below `.map-location`).
- `.map-location` — absolutely positioned (or CSS-grid placed) fixed-size box;
  reuse the existing border/padding idiom from `.investigator`.
- token classes (`.inv-token`, `.enemy-token`) — compact rows inside a node.

No Tailwind, no inline styles (matches the existing plain-CSS approach).

## Testing

wasm-bindgen-test (`crates/web/tests/`), using the existing `render_state` helper
(mount component, inject state via `reduce(Hello{..})`, `tick().await`, assert on
`inner_html()`). New cases:

- **Location nodes render** — a state with two connected locations renders both
  names in the map.
- **Investigator placed in its location** — an investigator at location A renders
  its token inside A's node (assert the token markup appears within the A box, e.g.
  via a per-node container class + the name).
- **Unengaged enemy in its location** — an enemy at A with `engaged_with == None`
  renders in A's node.
- **Engaged enemy in the detail panel, not the node** — an enemy `engaged_with`
  the investigator renders in that investigator's detail panel and is **absent**
  from the location node.
- **Connection lines render** — assert ≥1 SVG `<line>` for a two-connected-location
  state (and none for an isolated single location).
- **Fallback for an off-table code** — a location whose code isn't in the table
  still renders (no panic, name present).

## Non-goals (explicit)

- **Interactivity** — the board stays read-only; clicking a node/token does
  nothing; all actions remain on the `AwaitingInput` menu. (A future "click the
  map to act" step is out of scope.)
- **Engine layout hints** — coordinates stay client-side; no `GameState`/scenario
  changes. Promote to engine-provided hints only when a scenario needs authored
  geometry the client can't reasonably table.
- **Map background image, draggable nodes, zoom/pan.**
- **Set-aside location preview** — only in-play locations render.

## Open questions

None outstanding — the brainstorm settled the layout (positioned containers +
drawn lines), the node/detail split (token-in-node + detail panel), engaged-enemy
placement (detail panel), and the coordinate source (client-side table + fallback).
