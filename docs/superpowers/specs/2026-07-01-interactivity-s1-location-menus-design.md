# Interactivity S1 — web plumbing + location context menus — design

**Date:** 2026-07-01
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); slice S1 of the board interactivity pass
**Issue:** #536 · **Umbrella:** #206 · **Depends on:** S0 (#535, merged)
**Umbrella design:** `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Section 2)

## Goal

Stand up the shared web interactivity plumbing and prove it on **locations**: a map
node the active investigator can Move to / Investigate at **glows**, and clicking it
opens a **context menu** of those actions that submits the chosen `ResolveInput`.
First consumer of S0's `OptionTarget` anchor.

## Scope decisions (settled in brainstorm)

- **Per-entity inline menus, shared component.** Each entity owns its menu placement
  (the map node embeds its own `ContextMenu`), but the menu itself is one reusable
  `ContextMenu` component — per-entity positioning without duplicating open/submit/
  dismiss logic across the later Card/EnemyCard slices.
- **The flat action bar is untouched.** Per the transition choice, `input.rs` / the
  `.action-bar` keep rendering every open-turn option (including locations) until S6
  deletes the bar. So S1 is **purely additive** on the map side; a location's actions
  appear both on the map menu and in the bar during S1–S5 (accepted interim
  duplication). No `input.rs` change.
- **No prompt banner in S1** — deferred to the slice that first needs it (S3 Confirm /
  S4 Pass); during transition the residual bar hosts prompt text + Confirm/Pass.
- **No engine change.** `options_for` filters by anchor with a linear scan (option
  counts are tiny), so `OptionTarget` needs no `Hash`.
- **Even a single option opens the menu** (umbrella decision — player agency; no
  click-to-auto-execute).

## Architecture

### New module `crates/web/src/interaction.rs`

The interactivity plumbing, kept out of `map.rs`/`card.rs` so those stay focused.

- `pub fn pending_options(state: &ClientState) -> Vec<ChoiceOption>` — the live
  `AwaitingInput` request's `options`, else empty (`Done`/`Rejected`/no outcome →
  empty). Pure, native-testable.
- `pub fn options_for(options: &[ChoiceOption], target: OptionTarget) -> Vec<ChoiceOption>`
  — the options whose `target == target`, by linear filter. Pure, native-testable.
- `#[derive(Clone)] pub struct PendingOptions(pub Signal<Vec<ChoiceOption>>)` — the
  context newtype carrying the derived pending-options signal (a distinct type so it
  can't collide with other `Signal` contexts).
- `#[component] pub fn ContextMenu(options: Vec<ChoiceOption>, open: RwSignal<bool>)`
  — when `open()` is true, renders a full-screen transparent `.menu-backdrop`
  (`on:click` → set `open` false; the standard no-document-listener dismiss) plus a
  `.context-menu` with one `<button>` per option. An option click reads
  `OutboundTx` + the store from context, sets `store.pending_label = Some(label)`,
  sends `ClientMessage::Submit { PlayerAction::ResolveInput { PickSingle(id) } }`, and
  sets `open` false — mirroring today's `input.rs` submit path (which S6 folds into
  this). Reads `OutboundTx` as `Option` (absent in render-only/test contexts).

### `crates/web/src/map.rs` — node wiring

Inside the existing per-location `.map(...)` closure:
- Read `use_context::<PendingOptions>()` (empty when absent — existing `tests/map.rs`
  degrades gracefully to no menus).
- `let menu_opts = interaction::options_for(&pending, OptionTarget::Location(loc.id));`
- If `!menu_opts.is_empty()`: append `" actionable"` to `node_class`, attach
  `on:click` toggling a per-node `let open = RwSignal::new(false)`, and render
  `<ContextMenu options=menu_opts open/>` as a node child. (Per-node `open` is
  recreated on each board re-render, so an applied action closes the menu.)

### `crates/web/src/app.rs` — context provision

Where the tree is assembled (wasm-only, alongside the transport start):
`let pending = Signal::derive(move || interaction::pending_options(&store.get()));
provide_context(interaction::PendingOptions(pending));` — one provider; every current
and future entity reads it.

### `crates/web/src/lib.rs`

`mod interaction;` (wasm-gated consistent with the other view modules).

### `crates/web/style.css`

- `.map-location.actionable { box-shadow: 0 0 0 2px <accent>; cursor: pointer; }`
- `.context-menu { position: absolute; z-index: 20; … }` (a small vertical button stack)
- `.menu-backdrop { position: fixed; inset: 0; z-index: 15; }`

## Data flow

`store.outcome` (an `AwaitingInput` with S0-anchored options) → `pending_options`
(derived signal, provided as `PendingOptions`) → each map node filters via
`options_for(Location(id))` → glow + `ContextMenu` → option click → `OutboundTx`
submit `ResolveInput(PickSingle(id))` → server → new `Applied` → store updates → board
re-renders (menu closes).

## Testing

- **Native** (`interaction.rs` `#[cfg(test)]`): `pending_options` returns the request's
  options for `AwaitingInput` and empty for `Done`/no-outcome; `options_for` returns
  only the matching-anchor options and excludes `Global`, other-location, and enemy
  anchors.
- **Headless** (extend `crates/web/tests/map.rs`; synthetic registry is fine — the menu
  needs only a `Location`-anchored option + a node, not real metadata): with a
  `PendingOptions` carrying `Location(id)` options and a capturing `OutboundTx`
  provided, the target node carries the `actionable` class; clicking it reveals a
  `.context-menu` button; clicking that button sends
  `ResolveInput(PickSingle(id))`. A node whose location has no matching option has
  neither the class nor a menu.

Reuse the existing `tests/map.rs` mount + the submit-capturing `OutboundTx` pattern
from `tests/input.rs`.

## What "done" looks like (this slice)

- A Move/Investigate-legal location glows; clicking it opens a context menu of those
  actions; selecting one performs it (server round-trip).
- A non-actionable location is unchanged (no glow, no menu).
- The flat action bar still works unchanged (locations also still listed there).
- `interaction.rs` native tests + the `tests/map.rs` headless menu test pass; full
  7-job CI gauntlet green.

## Out of scope (later slices)

- Enemies (S2), hand + multi-select (S3), in-play/threat + window triggers (S4),
  act/soak/effect-choices (S5), global-action homes + bar retirement (S6).
- The prompt banner (S3/S4).
- "Only one menu open at a time" coordination across entities — trivially satisfied in
  S1 (locations only); revisited when a second entity type gets menus (S2) if needed.
- Menu positioning refinements (cursor-follow, edge-flip) — the node-anchored absolute
  placement suffices for S1.
