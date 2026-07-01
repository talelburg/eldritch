# Interactivity S2 — enemy context menus + fixed-at-cursor menu — design

**Date:** 2026-07-01
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); slice S2 of the board interactivity pass
**Issue:** #537 · **Umbrella:** #206 · **Depends on:** S1 (#536, merged)
**Umbrella design:** `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Section 2)

## Goal

Engaged enemies get their combat actions on the board: an enemy card the active
investigator can act on **glows** and opens a **context menu** of the engine's
offered verbs (Fight / Evade), submitting the chosen `ResolveInput`. Along the
way, upgrade the shared `ContextMenu` to open **at the cursor** (`position:
fixed`), which resolves the menu-clipping the S1 review flagged and makes every
entity type behave identically.

## Why the fixed-at-cursor change (the escape decision)

S1's menu is positioned `absolute` inside its anchor, which only worked because
`.map-location` is `position: absolute` (a positioning context) — and it is
clipped by that node's `overflow: hidden` (the S1 `TODO(#206)`). `.card` sets
neither `position` nor `overflow`, so an absolute menu inside an enemy card would
escape to the wrong ancestor. Rather than patch per-anchor, the menu moves to
`position: fixed` at the click's viewport coordinates: fixed is viewport-relative,
so it escapes *every* `overflow`/positioning ancestor uniformly, reads naturally
(menu at the pointer), and is paid for once in the shared component (S3/S4
inherit it). Alternatives weighed and rejected: per-anchor `position: relative` +
dropping the map's clip (menu pins to a fixed corner, node text can spill); a
leptos `Portal` (strictly heavier than fixed for the same result).

## Architecture

### `crates/web/src/interaction.rs`

- **`ContextMenu.open` changes type: `RwSignal<bool>` → `RwSignal<Option<(i32, i32)>>`.**
  `None` = closed; `Some((x, y))` = open at viewport coords `(x, y)`. When
  `open()` is `Some`, render the `.menu-backdrop` (click → `open.set(None)`) and a
  `.context-menu` positioned `position: fixed; left: {x}px; top: {y}px` (inline
  style). Each item click still submits `ResolveInput(PickSingle(id))` and sets
  `open` to `None`. Both backdrop and items keep `stop_propagation`.
- **New wasm-only helper `menu_layer(options: Vec<ChoiceOption>, open: RwSignal<Option<(i32, i32)>>) -> impl IntoView`.**
  Renders the trigger uniformly: a transparent hit-layer
  (`<div class="menu-hit">`, `position: absolute; inset: 0`) whose `on:click`
  captures `ev.client_x()` / `ev.client_y()` and sets `open = Some((x, y))`, plus
  the `<ContextMenu options open/>`. wasm-only because reading coords needs
  `web_sys::MouseEvent` (the `MouseEvent` web-sys feature — add to
  `crates/web/Cargo.toml`'s wasm-target `web-sys` features if not already pulled
  by leptos). The anchor's `actionable` glow class and `position: relative` stay
  non-gated. This DRYs the trigger across every entity.

### `crates/web/src/map.rs` (S1 update)

The location node adopts the new API: its per-node `open` becomes
`RwSignal::new(None::<(i32, i32)>)`, the old non-gated bool-toggle `on:click` is
**removed** (the hit-layer inside `menu_layer` now captures the open click), and
the wasm-only child becomes
`actionable.then(|| crate::interaction::menu_layer(menu_opts, open))`. The node is
already `position: absolute`, so the hit-layer sits correctly. This deletes the
`overflow` / `TODO(#206)` concern — the fixed menu escapes the node's
`overflow: hidden`.

### `crates/web/src/enemy_card.rs`

`EnemyCard` gains the same seam. It reads `use_context::<PendingOptions>()`,
computes `menu_opts = options_for(&pending, OptionTarget::Enemy(enemy.id))`, and
when non-empty adds `actionable` to the root class (so `.card.actionable` applies
its glow + `position: relative`) and embeds
`{ #[cfg(target_arch = "wasm32")] actionable.then(|| crate::interaction::menu_layer(menu_opts, open)) }`
with a per-card `open = RwSignal::new(None::<(i32, i32)>)`. Menu items are the
engine's labels (Fight / Evade / Engage — whichever it offers for that enemy). The
existing `enemy_stat_chips` / `enemy_keyword_chips` and display markup are
unchanged.

### `crates/web/style.css`

- `.card.actionable { position: relative; box-shadow: 0 0 0 2px #e0b84c; cursor: pointer; }`
  — a shared card glow (reused by S3/S4).
- `.menu-hit { position: absolute; inset: 0; z-index: 5; }` — transparent
  click-capture layer.
- `.context-menu` loses its `position: absolute; top/right` and becomes
  `position: fixed` (coords supplied inline); other properties (z-index, layout,
  colors) unchanged. `.map-location.actionable`'s glow stays; the map keeps
  `overflow: hidden` (the fixed menu escapes it anyway).

## Scope

- **In scope:** `EnemyCard` (board.rs renders it for engaged / threat-area
  enemies) gets Fight/Evade/Engage menus; the shared fixed-at-cursor upgrade;
  the S1 map call-site migration.
- **Deferred:** map-node **enemy tokens** (co-located, unengaged enemies render as
  terse tokens, not `EnemyCard`) getting their own Engage menu. In 1-player this
  is rare — enemies auto-engage on entry, so an `EnemyCard`'s engine offer is
  Fight/Evade; Engage targets non-you-engaged enemies, which in 1p are the map
  tokens. A small follow-up on a different render path; noted, not built.
- **Unchanged:** the flat action bar (still lists everything until S6); no prompt
  banner; no engine change.

## Testing

- **Native:** no new pure fns (`options_for` / `pending_options` already covered).
  The `enemy_stat_chips` / `enemy_keyword_chips` tests stay green.
- **Headless — `crates/web/tests/enemy_card.rs`:** mount `EnemyCard` directly with
  `PendingOptions` carrying an `Enemy(id)`-anchored option and a capturing
  `OutboundTx`; assert the card gets `actionable`, clicking it opens a
  `.context-menu` whose item text is the offered verb, and clicking that item
  submits `ResolveInput(PickSingle(id))`; an enemy with no matching option stays
  inert. (`.tc-root`/last-card scoping for DOM accumulation, per S1.)
- **Headless — refactor regression:** S1's `crates/web/tests/map.rs` menu test is
  updated to the new `Option<coords>` API and must still pass (an anchored
  location glows, opens its menu, submits `PickSingle`).
- **`ContextMenu` direct test (`tests/context_menu.rs`):** updated to construct
  `open` as `RwSignal::new(Some((0, 0)))` / `None` and still assert open→buttons,
  click→submit+close (`open` becomes `None`), closed→nothing.

## What "done" looks like (this slice)

- An engaged enemy the investigator can Fight/Evade glows; clicking it opens a
  cursor-anchored menu of those verbs; selecting one performs it (server
  round-trip). A non-actionable enemy is inert.
- The menu escapes `overflow:hidden` everywhere (locations included) — the S1
  clipping `TODO(#206)` is resolved.
- Flat action bar unchanged; hand / in-play / act still bar-only.
- Native + headless tests pass; full 7-job CI gauntlet green.

## Out of scope (later slices)

- Hand + multi-select (S3), in-play/threat + window triggers (S4),
  act/soak/effect-choices (S5), global-action homes + bar retirement (S6).
- Map-token (unengaged) enemy menus (deferred, above).
- "Only one menu open at a time" across entities — with S2 a second entity type
  gains menus, so two per-entity `open` signals can be `Some` at once. Harmless
  (opening a new menu's backdrop/selection closes via re-render on the resulting
  action; and a stray open menu is dismissed by its backdrop). Revisit only if it
  reads badly in practice.
- The prompt banner (S3/S4).
