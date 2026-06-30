# Enemy card rendering — design

**Date:** 2026-06-30
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); third slice of the zone-by-zone card-rendering rework

## Goal

Render **engaged enemies** as card rectangles showing their combat stats
(fight / evade / health / attack), keywords (Hunter / Retaliate / Victory),
traits, ability text, and exhausted state. Display-only.

Third slice of the card-rendering rework. Slice 1 ([#519](2026-06-29-web-card-rendering-design.md))
shipped the `Card` component for hand cards; slice 2
([#521](2026-06-30-in-play-card-rendering-design.md)) extended it for in-play
assets. This slice adds a **separate `EnemyCard` component** for enemies, which
are a different data source.

## Scope decisions

- **Display-only.** No click handlers; fighting/evading still goes through the
  existing controls.
- **Engaged enemies only.** Enemies engaged with an investigator, rendered in
  that investigator's threat area. Unengaged enemies at locations stay as the
  map's compact tokens until the locations/map slice redesigns the nodes.
- **Separate component, not `Card`.** The `Card` component is built around a
  `CardCode` → registry-metadata lookup plus an optional `CardInPlay`. An
  `Enemy` is a different data source: the `Enemy` *state* struct already carries
  name, traits, stats, and live state. So enemies get a dedicated `EnemyCard`
  that reads from `&Enemy`, sharing the card CSS / chip vocabulary and the text
  renderer rather than bending `Card`.
- **Treacheries stay text.** The threat area also holds treachery cards
  (`threat`); those keep their text `<ul>` (the threat-area slice handles them).

## Architecture

### New component `crates/web/src/enemy_card.rs`

- **`#[component] pub fn EnemyCard(enemy: Enemy)`** — renders a
  `.card .card--enemy` rectangle (red border so it reads as a threat), reusing
  the asset slice's `card--exhausted` dim + an `Exhausted` badge.
- **Two pure helpers** (native-testable, mirroring `live_state_chips`):
  - `enemy_stat_chips(enemy: &Enemy) -> Vec<String>` →
    `["fight {fight}", "evade {evade}", "health {damage}/{max_health}",
    "attack: {attack_damage} dmg · {attack_horror} hor"]`.
  - `enemy_keyword_chips(enemy: &Enemy) -> Vec<String>` → `"Hunter"` when
    `enemy.hunter`, `"Retaliate"` when `enemy.retaliate`, `"Victory {n}"` when
    `enemy.victory` is `Some(n)`; in that order; `[]` when none apply.
- **Ability text reuse:** `EnemyCard` looks the enemy's printed text up by
  `enemy.code` via the registry (`metadata_for`), rendering it with the existing
  `parse_card_text` + `render_segments`. To share those, `render_segments` is
  promoted from private to `pub(crate)` in `card.rs` (`parse_card_text` is
  already `pub`). Text is simply absent when no metadata exists.
- `Enemy` is `Clone` and `#[non_exhaustive]`; the prop takes `enemy: Enemy` by
  value (like `Card`'s `in_play`), so the component carries
  `#[allow(clippy::needless_pass_by_value)]`.
- Registered via `pub mod enemy_card;` in `lib.rs`.

### Board integration (`crates/web/src/board.rs`)

The `engaged` builder (currently maps each engaged enemy to
`<li class="enemy-engaged">…</li>`) becomes a `Vec` of
`<crate::enemy_card::EnemyCard enemy=e.clone()/>`. The threat-area container
(`<ul>{threat}{engaged}</ul>`) splits: treacheries stay in the `<ul>`, engaged
enemies render in a sibling `.card-row`:

```
<div class="threat">
  <h4>"Threat area"</h4>
  <ul>{threat}</ul>
  <div class="card-row">{engaged}</div>
</div>
```

The map's enemy-tokens (`map.rs`) are untouched.

### Styling (`crates/web/style.css`)

- `.card--enemy { border-color: #a3261b; }` (red), reusing `.card` / `.chip` /
  `.card--exhausted`. Optional chip tints (`.chip--enemy-stat`, `.chip--keyword`)
  are cosmetic; keep minimal.

## Field rendering summary (engaged enemy)

| Element | Source | Rendering |
|---|---|---|
| Name | `enemy.name` | header |
| Traits | `enemy.traits` | `"Monster. Ghoul."` |
| Ability text | registry `metadata_for(&enemy.code).text` | `parse_card_text` chips; absent if no metadata |
| Fight / Evade | `enemy.fight` / `enemy.evade` | chips `fight 3`, `evade 2` |
| Health | `enemy.damage` / `enemy.max_health` | chip `health 1/3` |
| Attack | `enemy.attack_damage` / `enemy.attack_horror` | chip `attack: 1 dmg · 1 hor` |
| Keywords | `enemy.hunter`, `enemy.retaliate` | chips `Hunter`, `Retaliate` (only when true) |
| Victory | `enemy.victory` | chip `Victory n` (only when `Some`) |
| Exhausted | `enemy.exhausted` | `card--exhausted` dim + `Exhausted` badge |
| Border | — | `card--enemy` (red) |

## Testing

- **Pure native tests** (in `enemy_card.rs`):
  - `enemy_stat_chips`: a `test_enemy` fixture with `fight`/`evade`/`damage`
    set → the four stat strings in order.
  - `enemy_keyword_chips`: hunter + retaliate + `victory: Some(2)` → `["Hunter",
    "Retaliate", "Victory 2"]`; a plain `test_enemy` → `[]`.
- **Headless wasm test** (`crates/web/tests/enemy_card.rs`, new,
  `#![cfg(target_arch = "wasm32")]`): an exhausted hunter + retaliate enemy
  (built from `test_enemy` + field mutation) → assert the `card--enemy` and
  `card--exhausted` classes, the `Exhausted` badge, the `fight`/`health 0/2`/
  `Hunter`/`Retaliate` chips, and the name. No registry install needed (the
  `_test_enemy_*` code has no metadata → ability text simply absent).
- **Board headless test** (`crates/web/tests/board.rs`): an investigator with an
  engaged enemy → assert `.threat .card-row .card` renders.

Tests construct `Enemy` via `game_core::test_support::fixtures::test_enemy`
(it is `#[non_exhaustive]`) + public-field mutation — never a struct literal.

## What "done" looks like (this slice)

- Engaged enemies render as `EnemyCard` rectangles (red border) with combat
  stats, keyword chips, traits, ability text, and a dimmed + badged appearance
  when exhausted.
- Hand cards and in-play assets are unchanged.
- Treacheries in the threat area, and the spatial map, still render as before.
- Native + headless tests pass; the full 7-job CI gauntlet is green.

## Out of scope (later slices)

- Enemies in the spatial map (locations/map slice will redesign nodes).
- Treachery cards in the threat area (threat-area slice).
- Locations and act/agenda card faces.
- `prey` display (moot in 1-player solo — always the sole investigator).
- Clickable / interactive enemies (fight/evade by clicking).
- The ArkhamDB icon font (still deferred; the chip seam is unchanged).
