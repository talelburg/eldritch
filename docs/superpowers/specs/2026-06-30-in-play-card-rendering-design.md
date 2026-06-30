# In-play card rendering — design

**Date:** 2026-06-30
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); second slice of the zone-by-zone card-rendering rework

## Goal

Render **in-play assets** as card rectangles that show the asset's printed face
*plus* its live per-instance state — exhausted/ready, uses remaining, and
soak (damage/horror accumulated on the card). Display-only.

This is the second slice of the card-rendering rework. The first slice
([#519 / PR #520](2026-06-29-web-card-rendering-design.md)) shipped the reusable
`Card` component for **hand** cards. This slice extends that component with an
optional in-play overlay and applies it to the in-play asset list. Threat area,
locations, enemies, and act/agenda remain later slices.

## Scope decisions

- **Display-only.** No click handlers; actions still flow through the existing
  controls/pickers (consistent with the hand slice).
- **In-play assets only.** `Investigator.cards_in_play`. Threat area
  (treacheries + engaged enemies), the spatial map (locations/enemies), and
  act/agenda are out of scope — each is its own later slice.
- **Extend, don't fork.** An in-play asset is the same Asset face the `Card`
  component already renders, plus live state. So `Card` gains an optional
  per-instance prop rather than spawning a parallel component.
- **Cost corner dropped for in-play.** A card in play has already been paid for,
  so the cost corner is omitted; the header space holds the exhausted badge.

## Architecture

### `Card` gains an optional in-play prop (`crates/web/src/card.rs`)

- New prop: `#[prop(optional)] in_play: Option<CardInPlay>`.
- **`None`** (hand path): unchanged from the first slice — cost corner, name,
  traits, text, slots, skill icons, fast/weakness markers.
- **`Some(inst)`** (in-play path): same printed face **minus the cost corner**,
  **plus**:
  - a `card--exhausted` class on the root (dimmed appearance) and an
    `EXHAUSTED` badge in the header marker slot, when `inst.exhausted`;
  - live-state footer chips (uses + soak), after the skill icons.

`CardInPlay` is `Clone`; the in-play call passes `code=c.code.clone()
in_play=c.clone()`. `code` stays required (the component is "a card identified by
code, optionally with in-play state"); the hand call site is untouched.

### New pure helper `live_state_chips` (`crates/web/src/card.rs`)

```
pub fn live_state_chips(inst: &CardInPlay, kind: &CardKind) -> Vec<String>
```

- **Uses:** one chip per `(UseKind, u8)` in `inst.uses`, formatted
  `"{count} {kind}"` with `kind` lowercased — `"2 ammo"`, `"3 supplies"`,
  `"1 charges"`, `"0 secrets"`. A `0` count still renders (depletion is visible).
- **Soak:** from `inst.accumulated_damage` / `inst.accumulated_horror` against the
  asset's capacity. Only when the asset (`CardKind::Asset { health, sanity, .. }`)
  has that capacity: `health: Some(h)` → `"dmg {accumulated_damage}/{h}"`;
  `sanity: Some(s)` → `"hor {accumulated_horror}/{s}"`. No chip when the
  capacity is `None` (non-ally asset).
- Order: uses chips, then soak chips. Returns `[]` for a plain asset with no
  uses and no soak capacity (e.g. Machete in play).
- Pure and native-testable, mirroring `slot_chips` / `skill_chips`. `UseKind`
  ordering follows `inst.uses` iteration (a `BTreeMap`, so deterministic).

### Board integration (`crates/web/src/board.rs`)

In `investigators_panel`, the in-play `<ul>` of `<li class="card-line">` items
becomes a `.card-row` of `<Card code=c.code.clone() in_play=c.clone()/>`. The
threat-area builder/container is unchanged. The now-dead `.in-play ul` CSS rule
is removed (mirrors the `.hand ul` cleanup from the first slice); `.threat ul`
stays.

### Styling (`crates/web/style.css`)

- `.card--exhausted` — dimmed: reduced opacity and a muted border so an
  exhausted card is visually distinct from a ready one. (Ready cards keep the
  full class-coloured border.)
- The exhausted badge reuses the `.card-fast` / `.card-weakness` header-marker
  styling; uses/soak chips reuse the existing `.chip` style. No new chip palette.

## Field rendering summary (in-play asset)

| Element | Source | Rendering |
|---|---|---|
| Cost corner | — | omitted |
| Name / traits / text / slots / skill icons | metadata | unchanged from hand face |
| Exhausted | `CardInPlay.exhausted` | `card--exhausted` dim + `EXHAUSTED` badge |
| Uses | `CardInPlay.uses` | chips: `"2 ammo"`, `"3 supplies"`, … |
| Soak | `CardInPlay.accumulated_damage/horror` vs asset `health`/`sanity` | chips: `"dmg 1/2"`, `"hor 0/2"` (only when capacity present) |

## Testing

- **Pure native test** for `live_state_chips`:
  - Beat Cop `01018` (`health: 2`, `sanity: 2`) with `accumulated_damage: 1` →
    `["dmg 1/2", "hor 0/2"]`.
  - An asset instance with `uses: {Ammo: 2}` → `["2 ammo"]`.
  - A plain asset (no uses, no soak capacity) → `[]`.
- **Headless wasm test** (`crates/web/tests/card.rs`):
  - In-play Beat Cop `01018`, `exhausted: true`, `accumulated_damage: 1`: assert
    the `card--exhausted` class, the `EXHAUSTED` badge text, the `dmg 1/2` chip,
    and that **no `card-cost`** element renders.
  - A non-exhausted in-play asset: assert no `card--exhausted` class.
- **Board headless test** (`crates/web/tests/board.rs`): the in-play section
  renders `.card-row .card` (scoped like the hand assertion), keeping the
  existing `_synth_asset` presence check.

All assertions read from the corpus/instance — no hand-typed stats. Card codes
verified against the snapshot (`data/arkhamdb-snapshot`): Beat Cop `01018`,
Machete `01020`.

## What "done" looks like (this slice)

- In-play assets render as `Card` rectangles with their printed face minus the
  cost corner, plus uses/soak chips, and a dimmed + badged appearance when
  exhausted.
- Hand cards are unchanged (the `None` path).
- Threat area, locations, enemies, act/agenda still render as before.
- Native + headless tests pass; the full 7-job CI gauntlet is green.

## Out of scope (later slices)

- Threat-area cards (treacheries like Cover Up, engaged enemies).
- Locations / enemies in the spatial map; act/agenda.
- Clickable / interactive cards (activating an asset by clicking it).
- The ArkhamDB icon font (still deferred; seam unchanged from the first slice).
- Attached cards (no in-scope asset carries attachments).
