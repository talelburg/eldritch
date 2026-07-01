# Interactivity S3 — hand Play menu + multi-select mode + prompt banner — design

**Date:** 2026-07-01
**Status:** approved (brainstorm), pending implementation plan
**Phase:** 7 (web iteration); slice S3 of the board interactivity pass
**Issue:** #538 · **Umbrella:** #206 · **Depends on:** S1 (#536), S2 (#537, both merged)
**Umbrella design:** `docs/superpowers/specs/2026-07-01-board-interactivity-pass-design.md` (Sections 2–3)

## Goal

Two things for the hand:
1. A playable hand card **glows** and opens a one-item **"Play …"** context menu
   (the S1/S2 pattern, anchored by `HandCard`).
2. The `PickMultiple` prompts (setup **mulligan**, skill-test **commit**, upkeep
   **hand-size discard**) become **click-to-select on the actual hand cards** plus
   a **Confirm** — introducing the deferred **prompt banner** (a bottom-fixed strip
   carrying prompt text + Confirm/Pass) and retiring the flat bar's commit UI.

## Decisions (settled in brainstorm)

- **Banner: a bottom-fixed strip** (near the hand — the Confirm sits by the cards
  you're selecting; mirrors where the action bar sits).
- **Move `PickMultiple` off the flat bar.** `input.rs`'s `PickMultiple` arm is
  removed; the board hand + banner replace it. Two selection UIs with separate
  state would be buggy, so — unlike the harmlessly-duplicated open-turn menu (S1/S2)
  — multi-select can't coexist. The bar keeps its `PickSingle`, `Confirm`, and
  `Skip` arms. A bounded, agreed deviation from "bar keeps everything until S6".
- **Single option still opens the menu** (Play is one item — umbrella rule).
- **Solo scope:** one investigator, so its hand is the selection surface;
  multi-investigator "only the prompted hand is selectable" is deferred.

## Architecture

### `crates/web/src/card.rs` — new `HandCardView` wrapper (`Card` unchanged)

`Card` stays display-only — it has two `view!` arms (asset/event vs generic) and is
already `#[allow(too_many_lines)]`; threading interactivity through both would bloat
it. Instead a focused wrapper owns the hand interaction:

`#[component] pub fn HandCardView(code: CardCode, investigator: InvestigatorId, index:
u8)` renders `<div class="hand-slot"> <Card code/> …interaction… </div>`. It reads
`PendingOptions` + the new `MultiSelect` context and branches on `multi_select.active`:
- **active (a `PickMultiple` is live) → selection mode.** No menu. The `.hand-slot`
  gets `class:selected` (reactive: `move || selected.get().contains(&index)`) and an
  `on:click` toggling `index` in `multi_select.selected`. This click is **non-gated**
  — it reads no coords (unlike the Play menu's `menu_layer`), just toggles a set, so it
  compiles on host (like S1's original node toggle). The reactive `class:selected`
  updates the ring on click without a re-mount.
- **inactive → Play menu.** `menu_opts = options_for(pending, OptionTarget::HandCard
  { investigator, hand_index: index })`; when non-empty, `.hand-slot` gets
  `class:actionable` + the wasm-only `menu_layer` (a "Play …" menu → `PickSingle`).

The two modes are mutually exclusive (a `PickMultiple` prompt is never the open turn).
`index` = the `OptionId(i)` hand-index convention `input.rs` already uses. The glow /
selection ring live on `.hand-slot` (which is `position: relative` so `menu_layer`'s
hit-layer anchors); `Card`'s markup is untouched.

### `crates/web/src/board.rs` — wrap hand cards

The hand render changes from `inv.hand.iter().map(|code| <Card code/>)` to
`.enumerate().map(|(i, code)| <HandCardView code investigator=inv.id index=u8::try_from(i)…/>)`.
In-play / threat `Card`s are unchanged (rendered bare).

### `crates/web/src/interaction.rs` — the `MultiSelect` context

```rust
#[derive(Clone)]
pub struct MultiSelect {
    /// True iff the live outcome is `AwaitingInput { kind: PickMultiple }`.
    pub active: leptos::prelude::Signal<bool>,
    /// The chosen hand indices; toggled by hand cards, read by the banner.
    pub selected: leptos::prelude::RwSignal<std::collections::BTreeSet<u32>>,
}
```

Plus a pure `pub fn is_multi_select(state: &ClientState) -> bool` (the `active`
derivation — matches `Some(AwaitingInput { request, .. })` with
`request.kind == InputKind::PickMultiple`), native-testable.

### `crates/web/src/prompt_banner.rs` (new, wasm-only) — `PromptBanner`

A bottom-fixed strip; submits via `OutboundTx`, so wasm-only (like `input.rs`).
When the live outcome is `PickMultiple`, renders `request.prompt` + a **Confirm**
(submits `ResolveInput(PickMultiple { selected: multi_select.selected.iter().copied()
.map(OptionId).collect() })`, then clears `selected`) + a **Pass** when
`request.skippable` (submits `ResolveInput(Skip)`). An effect clears `selected`
when `multi_select.active` goes false (each prompt starts empty). For S3 the banner
handles **only** `PickMultiple`; the encounter-draw `Confirm` and window `Skip`
stay in the flat bar until later slices.

### `crates/web/src/app.rs` — provide + mount

Provide `MultiSelect { active: Signal::derive(is_multi_select over store), selected:
RwSignal::new(BTreeSet::new()) }`. Mount `<PromptBanner/>` in the wasm-only block
(its bottom-fixed CSS places it regardless of DOM position).

### `crates/web/src/input.rs` — drop the `PickMultiple` arm

Remove the `InputKind::PickMultiple` match arm (the `active_hand` commit-hand UI +
its `selected` signal). The `PickSingle` / `Confirm` arms and the `Skip` control
remain. `active_hand` is deleted if now unused.

### `crates/web/src/lib.rs` / `style.css`

`pub mod prompt_banner;` (wasm-gated, like `input`). CSS: `.hand-slot { position:
relative; }`, `.hand-slot.actionable { box-shadow: 0 0 0 2px #e0b84c; cursor:
pointer; }`, `.hand-slot.selected { box-shadow: 0 0 0 3px #4a90d9; cursor: pointer; }`;
`.prompt-banner { position: fixed; bottom: 0; left: 0; right: 0; z-index: 25; … }`
(above the menu's z20 so it's never occluded).

## Testing

- **Native** (`interaction.rs`): `is_multi_select` — true for a `PickMultiple`
  outcome, false for `PickSingle` / `Confirm` / `Done` / none.
- **Headless — hand Play menu** (extend `crates/web/tests/card.rs`): a `HandCardView`
  whose `HandCard` anchor has an option shows `.hand-slot.actionable` and opens a
  "Play" menu that submits `PickSingle`; with no matching option it's inert; a bare
  in-play `Card` never glows.
- **Headless — selection mode + banner** (new `crates/web/tests/prompt_banner.rs`):
  under a `PickMultiple` outcome (with `MultiSelect` provided), clicking a
  `HandCardView` toggles `.hand-slot.selected`; the banner's Confirm submits
  `PickMultiple { selected }` with the toggled indices; a `skippable` `PickMultiple`
  shows Pass → `Skip`; selecting none → `PickMultiple { selected: [] }`.
- **Regression:** `input.rs` no longer renders the commit UI. Its `tests/input.rs`
  `PickMultiple` tests (`renders_prompt_and_hand_cards`, `mulligan_*`, `commit_*`,
  `hand_size_discard_*`, `pick_multiple_button_reads_confirm`) move to the
  banner/hand tests or are removed with the arm; the `PickSingle` / `Confirm` /
  `Skip` input tests stay.

Fixtures: reuse `awaiting_pick_single_with` (Play menu) and the existing
`awaiting_commit_input` / a `PickMultiple` fixture for selection mode; set the
store's `outcome` directly (no `GameState` needed — `is_multi_select` and
`pending_options` read `outcome`) as in the S2 enemy test.

## What "done" looks like (this slice)

- A playable hand card glows and opens a "Play" menu that plays it (server
  round-trip); a non-playable hand card is inert.
- During mulligan / commit / hand-size-discard, clicking hand cards toggles a
  selected ring, and the bottom-fixed banner's Confirm (or Pass) submits the
  selection; the flat bar no longer shows a commit UI.
- Native + headless tests pass; full 7-job CI gauntlet green.

## Out of scope (later slices)

- In-play/threat activate + window triggers (S4), act/soak/effect-choices (S5),
  global-action homes + bar retirement (S6).
- Multi-investigator selection scoping (deferred — solo only).
- The banner carrying non-`PickMultiple` prompts (encounter-draw `Confirm`, window
  `Skip`) — those stay in the flat bar until their slices / S6.
- No engine change; no `OptionTarget` change.
