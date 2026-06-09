# P6.7a — Core-loop action controls

**Issue:** [#188](https://github.com/talelburg/eldritch/issues/188) (Phase 6, slot P6.7a).
**Depends on:** P6.6 (`AwaitingInput` UI + `enabled_controls` legality helper, PR #207).
**Parent spec:** [`2026-06-08-phase-6-web-client-v0-design.md`](2026-06-08-phase-6-web-client-v0-design.md) (client layer 4).

## Goal

The action buttons that drive the synthetic toy scenario to a **Won**
resolution, built on P6.6's submit + legality plumbing. Acceptance: each
control, when legal, submits the correct `ClientMessage::Submit { action }`;
the toy scenario is clickable end-to-end to a Won state (Investigate →
clues → AdvanceAct).

## Module

New `crates/web/src/controls.rs`, wasm-only — declared
`#[cfg(target_arch = "wasm32")] pub mod controls;` in `lib.rs`, matching
`input` / `transport`. One component, `ActionControls`:

- Reads the store reactively via `use_store()`.
- Pulls `OutboundTx` from context as an `Option` (absent in render-only /
  test-without-channel contexts → clicks no-op), exactly as
  `AwaitingInputView` does.
- `board.rs` stays strictly read-only. All interactivity is isolated here.

## Gating

Each render computes `legality::enabled_controls(&game, &outcome)` (the
P6.6 helper). Every button binds its `disabled` to whether its
`ActionControl` is in that set. This is a **UX affordance, not a
correctness gate** — the server stays authoritative and rejects anything
illegal (P6.6 decision S2). When the store has no game (or no outcome),
the set is empty and everything is disabled.

## The seven controls

"active" investigator = `game.active_investigator`. Mulligan instead uses
the `mulligan_pending` cursor (the legality helper guarantees only
Mulligan is enabled in that window).

| Control | UI | Submitted `PlayerAction` |
|---|---|---|
| Investigate | button | `Investigate { investigator: active }` |
| EndTurn | button | `EndTurn` |
| DrawEncounterCard | button | `DrawEncounterCard` |
| AdvanceAct | button | `AdvanceAct { investigator: active }` |
| Move | one button per connected destination, from the active investigator's location `connections`, labeled by destination name | `Move { investigator: active, destination }` |
| PlayCard | a "Play" button per hand card (hand index = position) | `PlayCard { investigator: active, hand_index }` |
| Mulligan | own multi-select hand (`RwSignal<BTreeSet<u32>>`) + a submit button; empty selection = legal "keep" | `Mulligan { investigator: cursor, indices_to_redraw }` (indices downcast `u32` → `u8`) |

Each action is wrapped in `ClientMessage::Submit { action }` and pushed
onto the `OutboundTx` channel.

### Mulligan stays separate from the commit window

`AwaitingInputView` (P6.6) already has a multi-select-hand → submit shape,
and Mulligan is a second instance of it. They are **kept separate**: the
genuinely shared part (toggle a `BTreeSet`, render a hand of buttons) is
~12–15 lines, while the divergent parts are most of each component —
different index type (`u32` vs `u8`), different action
(`ResolveInput { CommitCards }` vs `Mulligan`), different gating
(`AwaitingInput` outcome vs the `mulligan_pending` cursor), different
investigator source, different label. Extracting now would also rewrite
shipped, tested P6.6 code to serve a new caller — wider blast radius than
the duplication saves. The clean extract trigger is a **third** concrete
use (the upkeep max-hand-size discard prompt, `InputResponse::Discard`,
deferred with #205); three real call sites will inform the shared shape.

## app.rs wiring

Add `<ActionControls/>` alongside `<AwaitingInputView/>` in the wasm-only
branch of `App`. **Remove `DebugSubmit`** — its doc-comment states "P6.7
builds the real action controls on this seam," so this change obsoletes
it (surgical cleanup of our own predecessor placeholder, not unrelated
refactoring).

## Edge handling

- **No game / no active investigator** → `enabled_controls` is empty, so
  every gated button is disabled. Move/PlayCard pickers render nothing
  (no location/hand to iterate).
- **Absent `OutboundTx`** → click handlers no-op (same `Option` guard as
  `AwaitingInputView`).

## Tests

`crates/web/tests/controls.rs`, `#![cfg(target_arch = "wasm32")]`,
mirroring `tests/input.rs`'s harness: mount `ActionControls` with a fresh
store + an `mpsc` outbound channel, feed a `Hello { state, outcome }`
through `reduce`, `tick()`, then read submitted frames off the receiver.
Because the headless runner shares one page and `mount_to_body` appends,
absence/selection assertions scope to the last-mounted subtree (board.rs
precedent).

One test per control — build a state where it is legal, click, assert the
exact `ClientMessage::Submit { action }` frame:

- Investigate (Investigation phase, active inv) → `Investigate { inv }`
- EndTurn → `EndTurn`
- AdvanceAct → `AdvanceAct { inv }`
- DrawEncounterCard (Mythos + `mythos_draw_pending`) → `DrawEncounterCard`
- Move (two connected locations) — click a destination → `Move { inv, dest }`
- PlayCard (hand with a card) — click a card's Play → `PlayCard { inv, hand_index }`
- Mulligan (`mulligan_pending` set) — select indices + submit →
  `Mulligan { inv, indices_to_redraw }`; and the empty/"keep" path
- One gating test: in a phase where a control is illegal, its button is
  `disabled` (and clicking submits no frame)

## Manual acceptance

Toy scenario clickable end-to-end to a **Won** state (Investigate →
accumulate clues → AdvanceAct). The Won/Lost *banner* is P6.8, so "Won"
is observed here via the resulting board state (act advanced), not a
dedicated resolution surface.

## Out of scope / follow-ups

- **Combat/edge controls** (Fight, Evade, Draw with enemy-target pickers)
  — P6.7b (#189).
- **Resolution banner** (Won/Lost surfacing) — P6.8 (#190).
- **Interactive board** — clicking board locations/hand cards directly to
  Move/PlayCard (rather than dedicated controls-panel pickers). Preferred
  for real player UX; deferred to a future phase. Recorded here as the
  brainstormed direction; this slot keeps `board.rs` read-only.
