# Investigator panel redesign — card + folded vitals beside the hand — design

**Date:** 2026-07-02
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration) · **Issue:** #547 · **Follows:** S4 (#539, merged)

## Goal

Rework the investigator panel's bottom zone so the **investigator card** is the home
for the character's live state — skills and health/sanity folded onto it, actions /
resources / clues / status beside it — sitting next to the hand. Display-only.

## Motivation

S4 (#539) rendered the investigator card under the name as an `InPlayCardView`
(so its reactions glow). This consolidates it: the card + the character's vitals in
one block by the hand, retiring the loose stats line. `location` drops (the map's
investigator token already shows where the character is).

## Layout (side-by-side; chosen from mockups)

The panel's bottom zone (`inv-zones-bottom`) becomes a two-column flex row:

```
Roland Banks
In play: [asset][asset]   Threat: [treachery]

┌──────────────┐ actions ●●●   │ Hand:
│ Roland Banks │ resources 5   │ [card][card]
│ W3 I3 C4 A2  │ clues 2       │ [card][card]
│ hp 0/9       │ status Active │
│ san 0/5      │               │
└──────────────┘               │
```

- **Left — investigator block** (`.investigator-block`, a flex row):
  - `.investigator-card`: the S4 `InPlayCardView` (unchanged — keeps its glow +
    reaction/Activate menu) with a **vitals footer** attached beneath it
    (`.inv-vitals`): the four skills `W{willpower} I{intellect} C{combat}
    A{agility}` and `hp {damage}/{max_health}` · `san {horror}/{max_sanity}`.
  - `.inv-meta`: the meta cluster beside the card — `actions` (rendered as
    `actions_remaining` filled pips), `resources {n}`, `clues {n}`, `status`.
- **Right — the hand** (`.hand`, the existing hand-card row), filling the remaining
  width.

## Architecture

- **`crates/web/src/board.rs`** — `investigators_panel`:
  - Remove the `.investigator-card` block that S4 placed under the `<h3 inv-name>`.
  - Remove the standalone `.inv-stats` `<span>` line (its data moves to
    `.inv-vitals` / `.inv-meta`; `location` is dropped).
  - Rebuild `.inv-zones-bottom` as `<div class="inv-zones-bottom"> <div
    class="investigator-block"> {investigator-card + .inv-vitals} {.inv-meta}
    </div> <div class="hand"> … </div> </div>`.
  - `.inv-vitals` is built from `inv.skills.{willpower,intellect,combat,agility}`,
    `inv.damage()/inv.max_health()`, `inv.horror()/inv.max_sanity()`. `.inv-meta`
    from `inv.actions_remaining`, `inv.resources`, `inv.clues`, `inv.status`.
  - `InPlayCardView` is reused untouched (the vitals/meta are panel-level siblings —
    that stat treatment is investigator-specific and doesn't belong in the generic
    wrapper).
- **`crates/web/style.css`** — `.inv-zones-bottom` becomes `display: flex` (block |
  hand); `.investigator-block` a flex row (card+vitals · meta); `.inv-vitals` a
  compact stat footer visually attached to the card (shared background/border feel);
  `.inv-meta` a small vertical cluster; `actions` pips. The old `.inv-stats` rule is
  removed.

## Data (all from `Investigator`)

| Shown | Source |
|---|---|
| skills W/I/C/A | `inv.skills.{willpower,intellect,combat,agility}` |
| hp `d/max` | `inv.damage()` / `inv.max_health()` |
| san `d/max` | `inv.horror()` / `inv.max_sanity()` |
| actions (pips) | `inv.actions_remaining` |
| resources | `inv.resources` |
| clues | `inv.clues` |
| status | `inv.status` (Debug) |
| the card | `inv.investigator_card` → `InPlayCardView` |

## Testing

- **Headless (`tests/board.rs` / extend):** with a built game, the panel renders the
  four skill values, `hp d/max`, `san d/max`, `actions`, `resources`, `clues`, and
  `status` in the investigator block, and the `.hand` renders beside it. The
  `location` line is gone.
- **Regression:** the S4 investigator-card reaction-glow test (`tests/map.rs`) still
  passes — the `.investigator-card .card-slot` still exists and glows for a
  `CardInstance`-anchored option (the card moved zones but is still an
  `InPlayCardView`).

## What "done" looks like

- The investigator card sits by the hand with skills + hp/san on it and
  actions/resources/clues/status beside it; no standalone stats line; no `location`
  line. The card still glows/opens its reaction/Activate menu. Full 7-job gauntlet
  green.

## Out of scope

- Any engine / interaction change (`InPlayCardView`, options, anchors untouched).
- Multi-investigator layout tuning beyond what the flex row gives for free (solo).
- Rendering skill *icons* / styling the card face itself (the skills are shown as
  values in the vitals footer, not via the `Card` component).
