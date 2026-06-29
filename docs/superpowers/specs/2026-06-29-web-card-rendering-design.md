# Web card rendering — design

**Date:** 2026-06-29
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web capstone / iteration); display-only first slice

## Goal

Replace the web client's text-only rendering of cards with visual **card
rectangles** that surface the information relevant to each card type — cost,
text, traits, skill icons, slots, health/sanity soak, shroud/clues, fight/evade,
keywords, and so on.

The end state covers every zone: hand, in-play, threat area, locations, act,
agenda, enemies. This is built **as a process, zone by zone**. This spec covers
the **first slice only: cards in hand**, plus the reusable component and styling
that every later zone inherits.

## Scope decisions

- **Display-only, this slice.** The cards render information; actions still flow
  through the existing controls/pickers. Making cards clickable (play a card by
  clicking it, move by clicking a location) is the eventual goal and the reason
  the component is built to be reused, but it is **out of scope here** — it builds
  on this foundation in a later slice. (This is consistent with the Phase-6
  decision that kept `board.rs` read-only and deferred board interactivity.)
- **First zone: hand.** Hand cards are the richest single card type and are
  self-contained in the investigator panel, so they establish the `Card`
  component, the markup-translation helper, and the CSS palette that later zones
  reuse. **In-play and threat area stay as text lists this slice** — they need
  per-instance state (uses, damage, attachments) that hand cards don't.
- **Layout: faithful mini-card.** Mirrors a real Arkham card — cost in a top
  corner, name, traits line, full text box, slots + skill icons along the
  bottom — colour-coded by class. Rectangles grow to fit text.

## Architecture

### New component: `crates/web/src/card.rs`

A reusable `Card` rectangle — the foundation every later zone reuses.

- **Input (this slice):** a `CardCode`. Later slices add a per-instance state
  argument (uses remaining, damage, etc.); the signature is designed to grow.
- **Metadata lookup:** via the installed registry, the same path `names.rs`
  uses — `game_core::card_registry::current().and_then(|r| (r.metadata_for)(code))`.
- **Fallback:** when metadata is missing (unimplemented stub, or registry not
  installed in a headless render path), render a bare rectangle showing just the
  raw code — mirrors `names::card_name`'s fallback. Never panics.
- **Output:** the faithful mini-card, colour-coded by `Class` via a CSS class
  (`card--guardian`, `card--seeker`, `card--rogue`, `card--mystic`,
  `card--survivor`, `card--neutral`, `card--mythos`).

This slice renders the card types that appear in **hand**: `Asset`, `Event`,
`Skill`. Other `CardKind`s (Location, Enemy, Act, Agenda, Treachery,
Investigator) get a minimal generic rectangle for now and are filled in by their
own later slices.

### Integration: `crates/web/src/board.rs`

In `investigators_panel`, the hand `<ul><li>{name}</li></ul>` becomes a flex row
of `Card` components, one per `CardCode` in `inv.hand`. The in-play and
threat-area lists are **unchanged** this slice.

### Styling: `crates/web/style.css`

CSS is a single `style.css` linked from `index.html` via
`<link data-trunk rel="css" …>`. Additions:

- the per-class colour palette (border/header colour keyed off `card--<class>`),
- the card box rules (rectangle, cost corner, traits line, text box, footer),
- the symbol/skill-icon **chip** styles (see markup translation).

## Field rendering (Asset / Event / Skill)

| Field | Source | Rendering |
|---|---|---|
| Cost corner | `Asset/Event.cost: Option<i8>` | number, or **X** when `None`; Skill shows no cost corner |
| Name | `metadata.name` | header |
| Traits | `metadata.traits` | joined, e.g. `"Item. Weapon. Melee."` |
| Text | `metadata.text` | translated markup (below) |
| Slots | `Asset.slots: Vec<Slot>` | footer chips: `Hand`, `Arcane×2`, … |
| Skill icons | `skill_icons: SkillIcons` | footer chips, one per non-zero of `willpower/intellect/combat/agility/wild` |
| Fast | `is_fast` | a "Fast" / ⚡ marker |
| Weakness | `metadata.weakness` | a red corner marker |

Stat fields come straight from `CardMetadata`/`CardKind` — never hand-typed.

## Card-text markup translation

Card text is stored with ArkhamDB markup. Observed forms (from
`crates/cards/src/generated/cards.rs`):

- `\n` line breaks
- `<b>…</b>`, `<i>…</i>` HTML emphasis
- `[[Tome]]`, `[[Item]]` — trait references (double bracket)
- `[combat]`, `[agility]`, `[intellect]`, `[willpower]`, `[wild]` — skill icons
- `[action]`, `[reaction]`, `[fast]`, `[free]` — ability-type icons
- `[elder_sign]`, `[skull]`, `[cultist]`, `[tablet]`, `[auto_fail]`,
  `[bless]`, `[curse]` — chaos-token icons

A pure helper `render_card_text(&str) -> impl IntoView` translates these:

- `\n` → line breaks
- `<b>…</b>` / `<i>…</i>` → styled spans
- `[[X]]` → the inner word `X`, emphasized
- known `[symbol]` → a small styled **abbreviation chip** (`<span>` with a
  per-symbol CSS class, e.g. `[combat]` → a `combat`-classed chip)
- unknown `[x]` → **left verbatim, brackets and all** (e.g. `[mystery]` renders
  as the literal text `[mystery]`). This is deliberate: an unmapped token should
  *pop out* in the UI so it gets noticed and we add a mapping, rather than
  silently degrading. Never crashes on a token we haven't mapped.

### Icon font: deferred, but the seam is built for it

This slice ships **text-abbreviation chips**, zero new assets. The real ArkhamDB
symbol font (an icomoon-style mapping of private-use codepoints to symbols) is a
nicer later polish but pulls in a vendored font asset.

The chip→glyph swap is designed to be a **clean drop-in, no restructuring**: the
token→view mapping in `render_card_text` and the footer skill-icon renderer are
the only two seams; the font swap changes *what each token renders to* (a glyph
span instead of a text chip), not the parsing, call sites, or test structure.

Folding in the font, if we choose to before merge, is mechanically routine —
`@font-face` + per-symbol classes in `style.css`, a `data-trunk` asset for the
`.woff2`, a token→codepoint table. **The real friction is provenance**, not code:
this repo is disciplined about vendored assets (cf.
`data/rules-reference/SOURCE.md`, the never-re-host-card-art rule), so the font
needs a clean source, a `SOURCE.md`-style provenance note, and a licence check.
**Decision deferred to near-merge:** confirm a clean font source then; if
provenance is murky, ship chips and revisit. Either way, no rework.

## Testing

- **Pure unit tests (native, off-wasm)** for `render_card_text` (each markup
  form: line breaks, `<b>`/`<i>`, `[[trait]]`, known symbols, unknown symbol) and
  the skill-icon / slot formatters. These need no DOM.
- **Headless `wasm-bindgen-test`** in `crates/web/tests/card.rs` (crate-level
  `#![cfg(target_arch = "wasm32")]`, per the established P6.3 pattern): render a
  real Asset (Machete `01020`) and a Skill, assert that cost, name, traits, text,
  and the icon chips are present in the DOM. The test installs `cards::REGISTRY`
  (idempotent `OnceLock`), as `names.rs`'s tests do.

## What "done" looks like (this slice)

- Hand cards render as faithful mini-card rectangles — cost, name, traits, full
  translated text, slots, skill icons — colour-coded by class.
- In-play and threat area still render as text (their slices come next).
- `render_card_text` translates every observed markup form; unknown tokens
  degrade gracefully.
- Native unit tests + the headless `card.rs` test pass; the full CI gauntlet is
  green.

## Out of scope (later slices)

- In-play / threat-area / enemy / location / act / agenda card rectangles
  (each its own slice, with per-instance state).
- Clickable cards / board interactivity.
- The ArkhamDB icon font (deferred; seam built for it).
- Card art (Phase-6 decision: link ArkhamDB CDN with text fallback; not this
  work).
