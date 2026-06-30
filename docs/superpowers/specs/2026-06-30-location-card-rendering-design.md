# Location card rendering — design

**Date:** 2026-06-30
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); fourth slice of the zone-by-zone card-rendering rework

## Goal

Evolve the spatial map's location nodes into **location cards** — showing name,
shroud, current clues, traits, ability text, and victory — and **normalize the
grid** so a removed location (the Study, post-Act-1) leaves no dead column.
Investigator and unengaged-enemy tokens render inside the nodes. Display-only.

Fourth slice of the rework. Slices 1–3 (merged) covered hand cards, in-play
assets, and engaged enemies. This slice covers the spatial map (locations + the
unengaged-enemy tokens deferred from the enemy slice).

## Scope decisions

- **Keep the spatial map.** Absolute-positioned grid nodes + connection lines
  stay (the spatial relationships are the point). Nodes grow into location cards.
- **Display-only.** No click handlers (moving still goes through the controls).
- **Tokens stay compact, not full cards.** Investigators and unengaged enemies
  inside a node render as compact colour-coded text lines, not `Card`/`EnemyCard`
  rectangles — they don't fit in a node, and engaged enemies already show full
  detail in the threat area.
- **Unrevealed locations withhold hidden info.** Only name + "unrevealed" +
  dashed/dimmed styling; shroud/clues/traits/text are not shown (matches the
  rules and current behaviour).
- **In scope:** map (`map.rs`) only. Threat-area treacheries and act/agenda are
  later slices.

## Architecture

### `crates/web/src/map.rs`

- **Grid normalization.** Today `layout_positions` returns authored cells as-is,
  so a removed location leaves its column reserved (the Study at col 0 → a dead
  left column once it leaves play). After computing the cell map, subtract the
  minimum assigned col and row so the placed nodes start at `(0, 0)`. This is a
  leading-offset normalization; collapsing *interior* empty columns is out of
  scope (no Core/Dunwich layout has interior gaps). Connection-line endpoints and
  `map_extent` derive from the same positions, so they follow automatically.
- **Larger nodes.** `CELL_W`/`CELL_H`/`NODE_W`/`NODE_H` grow so a node holds the
  card fields + a few tokens without clipping (`overflow: hidden` stays as a
  backstop). Exact values are an implementation detail tuned during build.
- **Nodes as location cards.** Each revealed node renders: name, a `shroud N`
  chip, `clues N` (current count), traits, ability text, and a `Victory n` chip —
  traits/text/victory looked up by `loc.code` via the registry
  (`metadata_for`), text rendered with `crate::card::parse_card_text` +
  `crate::card::render_segments` (the latter already `pub(crate)` from the enemy
  slice). Unrevealed nodes render only the name + "unrevealed".
- **Tokens inside.** Investigator tokens (green, `.inv-token`) show name + health
  + sanity + clues; unengaged-enemy tokens (red, `.enemy-token`) show name +
  `health d/m` + an exhausted marker when exhausted. Both reuse the existing
  classes, enriched.
- Node rendering stays inline in `location_map` (coupled to positioning); the
  normalization logic is a pure function, unit-tested.

### Styling (`crates/web/style.css`)

- `.map-location` grows and adopts the card vocabulary (it is already a bordered,
  rounded, white box); add the shroud/victory chips (reuse `.chip`) and spacing
  for the card fields. `.map-location.unrevealed` keeps its dashed/dimmed style.
- Token lines (`.inv-token` / `.enemy-token`) get minor spacing; colours stay.

## Field rendering summary (revealed location node)

| Element | Source | Rendering |
|---|---|---|
| Name | `loc.name` | header |
| Shroud | `loc.shroud` | chip `shroud N` |
| Clues | `loc.clues` | `clues N` (current count on the location) |
| Traits | registry `metadata.traits` | `"Passageway."` (empty string when none) |
| Ability text | registry `metadata.text` | `parse_card_text` chips; absent if no metadata/text |
| Victory | registry `metadata.victory` | chip `Victory n` (only when `Some`) |
| Investigator tokens | investigators at this location | compact green line: name + `d/maxhp` + `d/maxsan` + clues |
| Enemy tokens | unengaged enemies at this location | compact red line: name + `health d/m` (+ exhausted marker) |
| Unrevealed | `!loc.revealed` | name + "unrevealed" only; dashed/dimmed; no shroud/clues/text |

## Testing

- **Pure unit tests** (`map.rs`, native):
  - Grid normalization: `layout_positions` for the post-Study Gathering set
    (`Hallway 01112 → (2,1)`, `Attic 01113 → (2,0)`, `Cellar 01114 → (3,1)`,
    `Parlor 01115 → (2,2)`) returns positions whose minimum col is `0` and
    minimum row is `0` (the dead Study column is gone), while preserving relative
    offsets (Cellar one column right of Hallway/Attic/Parlor).
  - Existing `layout_positions` tests updated to expect normalized output.
- **Headless wasm test** (`crates/web/tests/map.rs`): a revealed location with a
  shroud, clues, an investigator at it, and an unengaged enemy at it renders the
  name, `shroud N`, `clues N`, the investigator token, and the enemy token; an
  unrevealed location renders its name but **not** its shroud/clues.

Stats read from the `Location`/`Enemy`/`Investigator` structs + corpus metadata
— never hand-typed. Codes verified against the snapshot (Gathering locations
`01111`–`01115`).

## What "done" looks like (this slice)

- The map's nodes render as location cards (name, shroud, clues, traits, text,
  victory) with compact investigator/enemy tokens inside; unrevealed nodes
  withhold hidden info.
- The grid is normalized — no dead column when the Study leaves play.
- Connection lines still connect the (repositioned) nodes.
- Hand/in-play/enemy cards unchanged.
- Native + headless tests pass; the full 7-job CI gauntlet is green.

## Out of scope (later slices)

- Threat-area treachery cards; act/agenda cards.
- Collapsing interior empty grid columns (no in-corpus layout needs it).
- Full `EnemyCard`/`Card` rendering inside map nodes (tokens stay compact).
- Clickable locations / map interactivity (the interactivity pass).
- The ArkhamDB icon font (still deferred).
