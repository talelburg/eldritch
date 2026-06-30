# Threat-area treachery card rendering — design

**Date:** 2026-06-30
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); fifth slice of the zone-by-zone card-rendering rework

## Goal

Render the treacheries in an investigator's threat area (Cover Up, Frozen in
Fear) as cards — name, traits, ability text, weakness marker, and clues-on-card —
alongside the engaged-enemy cards already there. Display-only.

Fifth slice. Slices 1–4 (merged) covered hand, in-play assets, engaged enemies,
and the location map. This finishes the threat area.

## Scope decisions

- **Reuse the `Card` component.** A threat-area treachery is a `CardInPlay`
  (code + per-instance state) — exactly `Card`'s model. `Treachery` falls through
  `card_face` to the **generic arm**, which already renders name + traits + text +
  weakness marker. No new component.
- **Clues-on-card is the only per-instance state.** Cover Up enters the threat
  area with 3 clues (`CardInPlay.clues`). Threat-area treacheries don't exhaust
  (no dim/badge), and attachments are out of scope.
- **Display-only.** No click handlers.
- **In scope:** threat-area treacheries (`inv.threat_area`). Act/agenda is the
  remaining zone (its own later slice).

## Architecture

### `crates/web/src/card.rs`

- **`live_state_chips` gains a clues chip.** Append `"clues {n}"` when
  `inst.clues > 0`, after the uses and soak chips. Generic — assets never carry
  clues; Cover Up does. Pure, native-tested.
- **The generic (`None`) arm renders the live-chip footer.** Today only the
  faithful-face (`Some`) arm renders the `card-live` chips. Add the same footer to
  the generic arm so a treachery's `clues N` chip shows. The arm keeps rendering
  name + weakness marker + traits + text; the footer is appended when the live
  chips are non-empty (`in_play` present with state). `exhausted` styling is
  **not** added to the generic arm (no in-scope treachery exhausts).

### `crates/web/src/board.rs`

The threat-area `threat` builder (currently `<li class="card-line">…</li>` in a
`<ul>`) becomes `<Card code=c.code.clone() in_play=c/>` cards. The threat
container drops the `<ul>` and renders treacheries + engaged enemies in one
`.card-row`:

```
<div class="threat">
  <h4>"Threat area"</h4>
  <div class="card-row">{threat}{engaged}</div>
</div>
```

The dead `.threat ul` CSS rule is removed (mirrors prior slice cleanups).

## Field rendering summary (threat-area treachery, generic arm)

| Element | Source | Rendering |
|---|---|---|
| Name | `metadata.name` | header |
| Weakness | `metadata.weakness` | red `Weakness` marker (Cover Up is a weakness) |
| Traits | `metadata.traits` | e.g. `"Task."` / `"Terror."` |
| Ability text | `metadata.text` | `parse_card_text` chips (Revelation / reaction / Forced) |
| Clues on card | `CardInPlay.clues` | chip `clues N` (only when `> 0`) |

A treachery with no installed metadata (synthetic `_synth_treachery`) renders via
`Card`'s existing raw-code fallback (still a `.card`) — the clues-chip rendering
is exercised against the real Cover Up.

## Testing

- **Pure native test** (`card.rs`): `live_state_chips` includes `"clues 3"` for a
  `CardInPlay` with `clues = 3` (e.g. on a Treachery kind, which has no uses/soak,
  so the result is exactly `["clues 3"]`); and no clues chip when `clues == 0`.
- **Headless wasm test** (`crates/web/tests/card.rs`, real `cards::REGISTRY`):
  a `Card` for Cover Up `01007` with `in_play.clues = 3` renders the name
  "Cover Up", a trait ("Task"), ability text ("Revelation"/"clues"), the
  `clues 3` chip, and carries the `card--generic` class (Treachery → generic arm).
- **Board headless test** (`crates/web/tests/board.rs`): an investigator with a
  treachery in `threat_area` renders a `.threat .card-row .card`.

Stats read from the corpus / `CardInPlay` — never hand-typed. Codes verified
against the snapshot: Cover Up `01007` (treachery/weakness, traits "Task.",
Revelation text, 3 clues), Frozen in Fear `01164` (treachery, traits "Terror.").

## What "done" looks like (this slice)

- Threat-area treacheries render as cards (name, traits, text, weakness marker,
  clues-on-card) in the threat `.card-row` alongside engaged-enemy cards.
- Hand / in-play / enemy / location cards unchanged.
- Native + headless tests pass; the full 7-job CI gauntlet is green.

## Out of scope (later slices)

- Act / agenda cards.
- Treachery exhaust/attachment state (no in-scope treachery uses them).
- Clickable cards / interactivity (the interactivity pass).
- The ArkhamDB icon font (still deferred).
